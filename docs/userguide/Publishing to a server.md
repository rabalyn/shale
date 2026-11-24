Shale pushes the site to a repository, and a small job on your server builds it and puts it in the folder your web
server serves. No Shale runs on the server, and the vault never goes there: the repository holds only the notes you
chose to publish and the files they show, as a Hugo site that needs nothing but `hugo` and `pagefind`. Back to
[[Publishing a vault]].

The job has the same four steps in both recipes below:

1. check out the branch Shale publishes to, **with Git LFS** (the audio and video files are LFS files, [[Publishing a vault]]);
2. run `hugo` (0.158 or newer, the Shale theme is inside the repository, nothing else is downloaded);
3. run `pagefind --site public` (the search box of the pages needs it);
4. put the result in place **in one step**, so that a visitor never sees half a site.

# With Forgejo Actions

For a Forgejo (or Gitea) that has Actions and a runner that can write to the folder your web server serves.

**The workflow cannot live on the published branch**: every publish replaces the branch with one snapshot that holds
only the site, so a workflow file of that branch would be gone at the next publish. Put the workflow in **a second
repository** (here `me/site-builder`, the file `.forgejo/workflows/site.yml`); it looks at the publish repository
(here `me/site`, the branch `site`) every few minutes and builds only when the branch has a new commit:

```yaml
name: Build the site
on:
  schedule:
    - cron: "*/5 * * * *"
  workflow_dispatch:

jobs:
  site:
    runs-on: docker
    steps:
      - name: Check out the published branch, with its LFS files
        uses: actions/checkout@v4
        with:
          repository: me/site
          ref: site
          lfs: true
          token: ${{ secrets.SITE_READ_TOKEN }}

      - name: Build the site and swap it in
        run: |
          set -eu
          releases=/srv/www/notes-releases
          sha=$(git rev-parse HEAD)
          if [ "$(cat "$releases/.built" 2>/dev/null || true)" = "$sha" ]; then
            echo "Nothing new since $sha"
            exit 0
          fi
          case "$(uname -m)" in
            aarch64 | arm64) hugo_cpu=arm64; pagefind_cpu=aarch64 ;;
            *) hugo_cpu=amd64; pagefind_cpu=x86_64 ;;
          esac
          curl -fsSL -o hugo.tar.gz "https://github.com/gohugoio/hugo/releases/download/v0.170.1/hugo_extended_0.170.1_linux-$hugo_cpu.tar.gz"
          tar -xzf hugo.tar.gz hugo
          curl -fsSL -o pagefind.tar.gz "https://github.com/Pagefind/pagefind/releases/download/v1.6.0/pagefind-v1.6.0-$pagefind_cpu-unknown-linux-musl.tar.gz"
          tar -xzf pagefind.tar.gz pagefind
          ./hugo --minify --destination public
          ./pagefind --site public
          release="$releases/$(date +%Y%m%d%H%M%S)-$$"
          mkdir -p "$release"
          cp -a public/. "$release"/
          ln -sfn "$release" /srv/www/notes.next
          mv -T /srv/www/notes.next /srv/www/notes
          echo "$sha" > "$releases/.built"
          ls -1d "$releases"/* | head -n -3 | xargs -r rm -rf
```

What it does: it checks out the branch with its LFS files, stops when that commit is the one it built last, builds
the site with Hugo and Pagefind (the build for the CPU of the runner, x86_64 or arm64), copies it to a new folder below `/srv/www/notes-releases` and points the symbolic
link `/srv/www/notes` at it with one rename, which is atomic. The web server's root is `/srv/www/notes`, and the three
newest builds are kept. Change `me/site` and `site` to the repository and the branch of **Settings → Publishing**;
make a token of a user who may read that repository and keep it as the secret `SITE_READ_TOKEN` of the builder
repository; and give the runner's label (`docker` here) the folder `/srv/www` as a volume. Where the runner may not
write there, let the last steps `rsync` the folder to the web server instead. A press on *Run workflow* builds at once.

# With a hook on a plain repository

When the publish repository is a plain bare repository on your own server (for example `ssh://me@server/srv/git/site.git`,
no web page, no Actions), a `post-receive` hook builds the site on every push. A plain repository has **no LFS store**,
so this recipe is for sites without audio and video: Shale stops when a site has them and the server cannot take LFS
files. Save this as `hooks/post-receive` in the bare repository and make it executable:

```sh
#!/bin/sh
# Builds the site when the branch "site" is pushed, and puts it in place in one step.
set -eu
branch=site
releases=/srv/www/notes-releases
repo=$(pwd)
unset GIT_DIR
work=
trap '[ -z "$work" ] || rm -rf "$work"' EXIT
while read -r _old _new ref; do
  [ "$ref" = "refs/heads/$branch" ] || continue
  work=$(mktemp -d)
  git clone --quiet --depth 1 --branch "$branch" "file://$repo" "$work/src"
  hugo --minify --source "$work/src" --destination "$work/public"
  pagefind --site "$work/public"
  release="$releases/$(date +%Y%m%d%H%M%S)-$$"
  mkdir -p "$releases"
  mv "$work/public" "$release"
  ln -sfn "$release" /srv/www/notes.next
  mv -T /srv/www/notes.next /srv/www/notes
  ls -1d "$releases"/* | head -n -3 | xargs -r rm -rf
  rm -rf "$work"
done
```

`hugo` and `pagefind` have to be in the `PATH` of the user who owns the repository. The web server's root is `/srv/www/notes`.
The process number in the name of a build keeps two builds that start within one second apart; a build that fails changes
nothing that is served and leaves no folder behind.

# Who may read the site

**Shale writes no login.** The site is **public** to everyone who can reach the web server, unless you protect it
there: put basic authentication or a single sign-on proxy (OpenID Connect, for example oauth2-proxy or Authelia) in front
of the folder. For basic authentication with nginx:

```
location / {
    root /srv/www/notes;
    auth_basic "Notes";
    auth_basic_user_file /etc/nginx/notes.htpasswd;
}
```

A site served below a path, such as `https://host.example/wiki/`, needs the **Address of the site** in Settings → Publishing
([[Publishing a vault]]) set to that address, so that its links go below the path; the same address gives the site its
`sitemap.xml`.

The search runs in the visitor's browser over files in the same folder, so the password protects the search too.
Keep the publish repository **private** on your server as well: whoever can read it can read the notes in it, with or
without the password of the site.

# What stays on the server

- A publish replaces the branch with one snapshot, so a note you stop publishing is gone from the branch and from its
  history at once. The folders of earlier builds that your job keeps (here: three) and the copies of the server's
  own backups are yours to clean.
- The audio and video files of a note that is no longer published stay in the LFS store of your server until the
  server collects the files that nothing uses; Forgejo does it on its schedule (the *LFS garbage collection*
  of its administration page).
- Pagefind's search index is made on the server, from the published notes only, and holds nothing else.
