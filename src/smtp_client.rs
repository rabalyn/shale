//! A small blocking SMTP client for sending one mail (ADR-0010, MAIL-121 to MAIL-123).
//!
//! It speaks what submitting a mail needs: `EHLO`, `STARTTLS`, `AUTH`, `MAIL FROM`, `RCPT TO`,
//! `DATA`, `RSET` and `QUIT`. The rules built into the one place that sends:
//!
//! - **Always TLS.** Implicit TLS from the first byte (465), or `STARTTLS` that must succeed
//!   before anything else, `AUTH` included, is sent (587). The certificate is checked by the
//!   operating system's verifier like the IMAP client's (the same configuration).
//! - **Typed commands.** The text of a command is made from typed values and passes
//!   [`check_command`] before it goes out: exact verbs, no line break, addresses without spaces,
//!   angle brackets or control characters.
//! - **The phase of a failure is known.** [`SmtpError::Unsure`] is returned only when the last byte
//!   of the message (the closing `.`) was written and the server's answer did not come: the mail
//!   may have been delivered. Everything before that is a definite "not sent"; so is a server that
//!   answers 4xx or 5xx to the end of the data. This is what lets the outbox never send twice
//!   without guessing (MAIL-123).
//! - **Bounded.** Lines, answers and waiting time have limits.
//!
//! Signing in goes through the same [`Authenticator`] hook as IMAP (password: `PLAIN`, else `LOGIN`;
//! OAuth2: `OAUTHBEARER` or `XOAUTH2`).

use crate::config::MailSecurity;
use crate::imap_client::{base64_encode, platform_tls_config, tcp_connect, tls_stream, Authenticator, ConnectSettings, ImapError, Io, Wire};
use rustls::ClientConfig;
use std::collections::HashSet;
use std::fmt;
use std::io::Write;
use std::sync::Arc;

/// What went wrong with a send, for a message to the person and for the outbox to decide on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmtpError {
    /// Could not reach the server, or the connection broke before the message was complete.
    Connection(String),
    /// TLS failed, such as a certificate that is not trusted.
    Tls(String),
    /// Wrong user name or password or token, or the server refused the sign-in.
    Auth(String),
    /// The server did not accept the sender, a recipient or the message (the words it said). Nothing was sent.
    Refused(String),
    /// The server said something this client does not understand.
    Protocol(String),
    /// The closing `.` of the message was written and no answer came: **the mail may have been delivered.**
    Unsure(String),
}

impl fmt::Display for SmtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmtpError::Connection(s) => write!(f, "connection: {s}"),
            SmtpError::Tls(s) => write!(f, "secure connection: {s}"),
            SmtpError::Auth(s) => write!(f, "sign-in failed: {s}"),
            SmtpError::Refused(s) => write!(f, "the server did not accept the mail: {s}"),
            SmtpError::Protocol(s) => write!(f, "unexpected answer from the server: {s}"),
            SmtpError::Unsure(s) => write!(f, "the server did not answer after the mail was sent: {s}"),
        }
    }
}

impl std::error::Error for SmtpError {}

pub type Result<T> = std::result::Result<T, SmtpError>;

fn from_imap(e: ImapError) -> SmtpError {
    match e {
        ImapError::Connection(s) => SmtpError::Connection(s),
        ImapError::Tls(s) => SmtpError::Tls(s),
        ImapError::Auth(s) => SmtpError::Auth(s),
        ImapError::Protocol(s) | ImapError::Server(s) | ImapError::Refused(s) => SmtpError::Protocol(s),
    }
}

/// One answer: the code and its lines of text (without the code).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub code: u16,
    pub lines: Vec<String>,
}

impl Reply {
    pub fn text(&self) -> String {
        self.lines.join(" ").chars().filter(|c| !c.is_control()).take(400).collect()
    }
}

/// The gate every command passes before it is sent (MAIL-122).
pub fn check_command(cmd: &str) -> Result<()> {
    let refuse = |why: &str| Err(SmtpError::Protocol(format!("refused to send a command: {why}")));
    if cmd.contains(['\r', '\n', '\0']) || cmd.len() > 600 {
        return refuse("a line break, NUL or length");
    }
    let addr_ok = |a: &str| !a.is_empty() && a.len() <= 320 && a.is_ascii() && !a.contains(|c: char| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | '"' | ',' | ';' | '\\'));
    let (verb, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match verb {
        "STARTTLS" | "DATA" | "RSET" | "QUIT" if rest.is_empty() => Ok(()),
        "EHLO" if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'[' | b']' | b':')) => Ok(()),
        "AUTH" => {
            let (mech, ir) = rest.split_once(' ').unwrap_or((rest, ""));
            let b64 = |s: &str| s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-'));
            if matches!(mech, "PLAIN" | "LOGIN" | "XOAUTH2" | "OAUTHBEARER") && b64(ir) {
                Ok(())
            } else {
                refuse("a malformed AUTH")
            }
        }
        "MAIL" => match rest.strip_prefix("FROM:<").and_then(|r| r.split_once('>')) {
            Some((a, tail)) if (a.is_empty() || addr_ok(a)) && (tail.is_empty() || (tail.starts_with(" SIZE=") && tail[6..].bytes().all(|b| b.is_ascii_digit()) && tail.len() > 6 && tail.len() < 20)) => Ok(()),
            _ => refuse("a malformed MAIL FROM"),
        },
        "RCPT" => match rest.strip_prefix("TO:<").and_then(|r| r.strip_suffix('>')) {
            Some(a) if addr_ok(a) => Ok(()),
            _ => refuse("a malformed RCPT TO"),
        },
        _ => refuse("not a command that sending uses"),
    }
}

/// One connection that has signed in. Sends one mail ([`Smtp::send`]) or just proves that the sign-in works.
pub struct Smtp {
    wire: Wire,
    caps: HashSet<String>,
    max_size: Option<u64>,
    secure: bool,
}

impl Smtp {
    /// Connect, secure the connection, say `EHLO` and sign in. The certificate is checked by the operating system.
    pub fn connect(settings: &ConnectSettings, auth: &mut dyn Authenticator) -> Result<Smtp> {
        Smtp::connect_with(settings, platform_tls_config().map_err(from_imap)?, auth)
    }

    /// Tests only: trust the pretend server's certificate in addition.
    #[cfg(test)]
    pub fn connect_test(settings: &ConnectSettings, root: rustls::pki_types::CertificateDer<'static>, auth: &mut dyn Authenticator) -> Result<Smtp> {
        Smtp::connect_with(settings, crate::imap_client::test_tls_config(root), auth)
    }

    fn connect_with(settings: &ConnectSettings, tls: Arc<ClientConfig>, auth: &mut dyn Authenticator) -> Result<Smtp> {
        let tcp = tcp_connect(settings).map_err(from_imap)?;
        let mut s = match settings.security {
            MailSecurity::ImplicitTls => Smtp::new(tls_stream(tls.clone(), &settings.host, tcp).map_err(from_imap)?, true, settings),
            MailSecurity::StartTls => Smtp::new(Io::Plain(tcp), false, settings),
        };
        s.greeting()?;
        s.ehlo()?;
        if !s.secure {
            if !s.caps.contains("STARTTLS") {
                return Err(SmtpError::Tls("the server does not offer STARTTLS, and a connection without encryption is never used".into()));
            }
            let r = s.command("STARTTLS")?;
            if r.code != 220 {
                return Err(SmtpError::Tls(format!("the server refused STARTTLS: {}", r.text())));
            }
            if s.wire.has_buffered() {
                return Err(SmtpError::Tls("data arrived before the encryption started".into()));
            }
            let Io::Plain(tcp) = std::mem::replace(&mut s.wire.io, Io::Gone) else { return Err(SmtpError::Tls("already encrypted".into())) };
            s.wire.io = tls_stream(tls, &settings.host, tcp).map_err(from_imap)?;
            s.secure = true;
            s.caps.clear(); // what the server offered before the encryption does not count
            s.ehlo()?;
        }
        s.authenticate(auth)?;
        Ok(s)
    }

    fn new(io: Io, secure: bool, settings: &ConnectSettings) -> Smtp {
        Smtp { wire: Wire::new(io, settings.limits.max_line.min(1 << 20)), caps: HashSet::new(), max_size: None, secure }
    }

    fn read_reply(&mut self) -> Result<Reply> {
        let mut lines = vec![];
        loop {
            let line = self.wire.read_line().map_err(from_imap)?;
            let text = String::from_utf8_lossy(&line).into_owned();
            let (code, more, rest) = match (text.get(..3).and_then(|c| c.parse::<u16>().ok()), text.as_bytes().get(3)) {
                (Some(c), Some(b'-')) => (c, true, &text[4..]),
                (Some(c), Some(b' ')) => (c, false, &text[4..]),
                (Some(c), None) => (c, false, ""),
                _ => return Err(SmtpError::Protocol("an answer that does not start with a code".into())),
            };
            lines.push(rest.to_string());
            if lines.len() > 200 {
                return Err(SmtpError::Protocol("too many lines in one answer".into()));
            }
            if !more {
                return Ok(Reply { code, lines });
            }
        }
    }

    fn greeting(&mut self) -> Result<()> {
        let r = self.read_reply()?;
        if r.code == 220 {
            Ok(())
        } else {
            Err(SmtpError::Refused(format!("the server did not greet: {} {}", r.code, r.text())))
        }
    }

    /// Send a command (it passes the gate) and read the answer.
    fn command(&mut self, cmd: &str) -> Result<Reply> {
        check_command(cmd)?;
        self.write_all(format!("{cmd}\r\n").as_bytes())?;
        self.read_reply()
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.wire.io.write_all(bytes).and_then(|_| self.wire.io.flush()).map_err(|e| from_imap(crate::imap_client::io_error(e)))
    }

    fn ehlo(&mut self) -> Result<()> {
        let r = self.command("EHLO localhost")?;
        if r.code != 250 {
            return Err(SmtpError::Refused(format!("the server did not accept EHLO: {} {}", r.code, r.text())));
        }
        self.caps.clear();
        self.max_size = None;
        for l in r.lines.iter().skip(1) {
            let up = l.to_ascii_uppercase();
            if let Some(n) = up.strip_prefix("SIZE ") {
                self.max_size = n.trim().parse().ok().filter(|n| *n > 0);
            }
            // `AUTH PLAIN LOGIN` becomes `AUTH=PLAIN`, `AUTH=LOGIN` (like IMAP's capabilities)
            if let Some(m) = up.strip_prefix("AUTH ").or_else(|| up.strip_prefix("AUTH=")) {
                for mech in m.split_whitespace() {
                    self.caps.insert(format!("AUTH={mech}"));
                }
            }
            self.caps.insert(up.split_whitespace().next().unwrap_or("").to_string());
        }
        Ok(())
    }

    pub fn has_capability(&self, c: &str) -> bool {
        self.caps.contains(&c.to_ascii_uppercase())
    }

    fn authenticate(&mut self, auth: &mut dyn Authenticator) -> Result<()> {
        if !self.secure {
            return Err(SmtpError::Tls("signing in on a connection without encryption is never done".into()));
        }
        {
            let caps = self.caps.clone();
            auth.select_mechanism(&|m| caps.contains(&format!("AUTH={}", m.to_ascii_uppercase())));
        }
        let mech = auth.mechanism().to_ascii_uppercase();
        if self.caps.contains(&format!("AUTH={mech}")) {
            return self.auth_sasl(&mech, auth);
        }
        // a password can also sign in with LOGIN
        if let (true, true, Some((user, pass))) = (mech == "PLAIN", self.caps.contains("AUTH=LOGIN"), auth.login_credentials()) {
            return self.auth_login(&user, &pass);
        }
        Err(SmtpError::Auth(format!("the server does not offer {mech} sign-in")))
    }

    fn auth_sasl(&mut self, mech: &str, auth: &mut dyn Authenticator) -> Result<()> {
        let initial = auth.initial_response();
        let mut r = self.command(&format!("AUTH {mech} {}", base64_encode(&initial)))?;
        let mut reason: Option<String> = None;
        for _ in 0..4 {
            match r.code {
                235 => return Ok(()),
                334 => {
                    let challenge = crate::mail::base64_decode(r.lines.first().map(|l| l.trim()).unwrap_or("").as_bytes());
                    if !challenge.is_empty() {
                        reason = Some(String::from_utf8_lossy(&challenge).chars().filter(|c| !c.is_control()).take(300).collect());
                    }
                    let answer = auth.respond(&challenge);
                    self.write_all(format!("{}\r\n", base64_encode(&answer)).as_bytes())?;
                    r = self.read_reply()?;
                }
                _ => break,
            }
        }
        Err(SmtpError::Auth(match reason {
            Some(why) => format!("{} {} (the server said: {why})", r.code, r.text()),
            None => format!("{} {}", r.code, r.text()),
        }))
    }

    fn auth_login(&mut self, user: &str, pass: &str) -> Result<()> {
        let r = self.command("AUTH LOGIN")?;
        if r.code != 334 {
            return Err(SmtpError::Auth(format!("{} {}", r.code, r.text())));
        }
        self.write_all(format!("{}\r\n", base64_encode(user.as_bytes())).as_bytes())?;
        let r = self.read_reply()?;
        if r.code != 334 {
            return Err(SmtpError::Auth(format!("{} {}", r.code, r.text())));
        }
        self.write_all(format!("{}\r\n", base64_encode(pass.as_bytes())).as_bytes())?;
        let r = self.read_reply()?;
        if r.code == 235 {
            Ok(())
        } else {
            Err(SmtpError::Auth(format!("{} {}", r.code, r.text())))
        }
    }

    /// Send one mail: `from` and `recipients` are the envelope (bare addresses), `data` the message
    /// exactly as it is to be delivered. Returns the server's words on success.
    ///
    /// Every failure before the closing `.` of the data is a definite "not sent". If any recipient is
    /// refused nothing is sent at all (no mail goes to only some of the people). After the closing `.`
    /// has been written, a missing answer is [`SmtpError::Unsure`].
    pub fn send(&mut self, from: &str, recipients: &[String], data: &[u8]) -> Result<String> {
        if recipients.is_empty() {
            return Err(SmtpError::Refused("no recipient".into()));
        }
        if has_bare_line_break(data) {
            return Err(SmtpError::Refused("the message has a line break that is not CR LF".into()));
        }
        if let Some(max) = self.max_size {
            if data.len() as u64 > max {
                return Err(SmtpError::Refused(format!("the message ({} bytes) is bigger than the {max} bytes this server accepts", data.len())));
            }
        }
        let size = if self.has_capability("SIZE") { format!(" SIZE={}", data.len()) } else { String::new() };
        let r = self.command(&format!("MAIL FROM:<{from}>{size}"))?;
        if !(200..300).contains(&r.code) {
            return Err(SmtpError::Refused(format!("the sender {from}: {} {}", r.code, r.text())));
        }
        for rcpt in recipients {
            let r = self.command(&format!("RCPT TO:<{rcpt}>"))?;
            if !(200..300).contains(&r.code) {
                // nothing has been sent: forget the transaction (the connection is dropped by the caller)
                let _ = self.command("RSET");
                return Err(SmtpError::Refused(format!("the recipient {rcpt}: {} {}", r.code, r.text())));
            }
        }
        let r = self.command("DATA")?;
        if r.code != 354 {
            let _ = self.command("RSET");
            return Err(SmtpError::Refused(format!("DATA: {} {}", r.code, r.text())));
        }
        let body = dot_stuff(data);
        // a failure while writing: the end of the message is not complete, so the server cannot have it
        self.write_all(&body)?;
        // from here on the server may have the whole message
        let r = match self.read_reply() {
            Ok(r) => r,
            Err(e) => return Err(SmtpError::Unsure(e.to_string())),
        };
        if (200..300).contains(&r.code) {
            Ok(r.text())
        } else {
            Err(SmtpError::Refused(format!("{} {}", r.code, r.text())))
        }
    }

    /// Say goodbye (errors do not matter any more).
    pub fn quit(mut self) {
        let _ = self.command("QUIT");
    }
}

/// A CR that is not followed by LF, or an LF that does not follow a CR.
pub fn has_bare_line_break(data: &[u8]) -> bool {
    data.iter().enumerate().any(|(i, &b)| (b == b'\r' && data.get(i + 1) != Some(&b'\n')) || (b == b'\n' && (i == 0 || data[i - 1] != b'\r')))
}

/// The data as it goes on the wire: a dot at the start of a line is doubled, the data ends with CR LF
/// and then `.` CR LF.
pub fn dot_stuff(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 40 + 8);
    let mut at_line_start = true;
    for &b in data {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = b == b'\n';
    }
    if !data.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b".\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: MAIL-123
    #[test]
    fn the_body_is_dot_stuffed_ends_with_a_dot_line_and_bare_line_breaks_are_found() {
        assert_eq!(dot_stuff(b"a\r\n.b\r\n..\r\n"), b"a\r\n..b\r\n...\r\n.\r\n");
        assert_eq!(dot_stuff(b".\r\n"), b"..\r\n.\r\n");
        assert_eq!(dot_stuff(b"no end"), b"no end\r\n.\r\n");
        assert_eq!(dot_stuff(b""), b"\r\n.\r\n");
        assert!(!has_bare_line_break(b"a\r\nb\r\n"));
        assert!(has_bare_line_break(b"a\nb"));
        assert!(has_bare_line_break(b"a\rb"));
        assert!(has_bare_line_break(b"\n"));
    }

    // spec: MAIL-122
    #[test]
    fn only_the_commands_of_sending_pass_the_gate_in_their_exact_shapes() {
        for ok in ["EHLO localhost", "STARTTLS", "AUTH PLAIN AGFkYQBwdw==", "AUTH XOAUTH2 dXNlcj0", "AUTH LOGIN", "MAIL FROM:<ada@example.com>", "MAIL FROM:<ada@example.com> SIZE=1234", "RCPT TO:<bob@x.de>", "DATA", "RSET", "QUIT"] {
            assert!(check_command(ok).is_ok(), "{ok}");
        }
        for bad in [
            "VRFY bob",
            "EXPN list",
            "MAIL FROM:<a@b.c>\r\nRCPT TO:<x@y.z>",
            "RCPT TO:<a@b.c> NOTIFY=NEVER",
            "RCPT TO:<a b@c.d>",
            "RCPT TO:<a@b.c,d@e.f>",
            "RCPT TO:<ü@b.c>",
            "RCPT TO:<>",
            "MAIL FROM:<a@b.c> BODY=8BITMIME",
            "MAIL FROM:<a@b.c> SIZE=",
            "DATA now",
            "AUTH CRAM-MD5",
            "AUTH PLAIN a b",
            "EHLO bad host",
            "HELO x",
            "",
            "STARTTLS\0",
        ] {
            assert!(check_command(bad).is_err(), "{bad:?}");
        }
        assert!(check_command(&format!("RCPT TO:<{}@x.de>", "a".repeat(400))).is_err());
    }
}
