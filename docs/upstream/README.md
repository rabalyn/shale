# Patches for other projects

Fixes for the crates and programs Shale uses that are meant to be sent to their authors. Shale
works around each one in its own code (so it is not blocked), and the workaround names the patch.
Nothing here is sent automatically; the owner decides when.

| Patch | For | Shale's workaround | Issue |
|---|---|---|---|
| `symphonia-riff-alaw-fmt16.patch` | `symphonia-format-riff` (pdeljanov/Symphonia): accept the 16-byte `fmt ` chunk of A-law and µ-law WAV files | `src/media/wavfix.rs` | #581 |

## Sending `symphonia-riff-alaw-fmt16.patch`

1. Fork <https://github.com/pdeljanov/Symphonia> and check out its current `master`.
2. Apply the part after the message (the paths start with `symphonia-format-riff/`), for example
   `git apply symphonia-riff-alaw-fmt16.patch` from the repository root (`git apply -C1` helps if
   the lines moved). Add a test: a WAV file with `WAVE_FORMAT_ALAW` and a 16-byte `fmt ` chunk
   (8 kHz, mono, 8 bits) must open and decode.
3. Commit with the first paragraph of the patch's header as the message, open a pull request and
   say which recorders write the short chunk (phone systems, FRITZ!Box call recordings).
4. When a release with the fix is Shale's dependency, delete the widening in `src/media/wavfix.rs`
   (keep `wav_format`, which names the file in error messages) and this patch.
