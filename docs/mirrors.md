# Building on a restricted network

Favnyr pins its toolchain and pulls its crates from crates.io, so a machine that
cannot reach `static.rust-lang.org` or `index.crates.io` fails before a single
line is compiled. Both hosts are frequently unreachable from mainland-China
links, where the TLS handshake never completes. This note records what the
repository does about it, and how to undo it where the network is unrestricted.

Neither mirror affects Favnyr at run time: the built application still makes no
network request. This is about the build only.

## What needs the network

- **The toolchain.** `rust-toolchain.toml` pins an exact channel (currently
  `1.97.1`) plus `rustfmt` and `clippy`. rustup downloads it on first use. The
  pin is deliberate — CI treats warnings as errors, so a floating channel would
  let a runner pick up a newer compiler than the development machine and fail a
  build that passed locally. See the comment in that file.
- **The crates.** The workspace resolves everything from crates.io.

## crates.io — the repository mirror

`.cargo/config.toml` at the repository root replaces the crates.io source with
the sparse mirror `https://rsproxy.cn/index/`. Cargo reads it automatically for
any command run inside the repository, so a fresh clone builds on a restricted
connection with no per-machine setup.

Trade-off: the file applies to *everyone* who builds this repository, including
GitHub Actions runners that reach crates.io natively and see the mirror only as
a slower detour. Any user-level `~/.cargo/config.toml` or `cargo --config` value
takes precedence, so a machine with normal connectivity can opt back out without
touching the repository, and deleting `.cargo/config.toml` from a checkout is
enough to build against crates.io again.

## rustup — the toolchain mirror

rustup has no config file for this; it reads two environment variables:

```sh
export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
```

Set them before the first `rustup`/`cargo` command that triggers the toolchain
download. On Windows, persisted for the current user:

```powershell
setx RUSTUP_DIST_SERVER "https://rsproxy.cn"
setx RUSTUP_UPDATE_ROOT "https://rsproxy.cn/rustup"
```

With both in place, a clean clone builds end to end:

```sh
cargo run --release --bin favnyr
```

## Other mirrors

The same two environment variables and the same `[source]` block work with any
crates.io / rustup mirror. The USTC mirror, for example, is
`https://mirrors.ustc.edu.cn/rust-static` for rustup and
`sparse+https://mirrors.ustc.edu.cn/crates.io-index/` for crates. Swap the URLs
if rsproxy is not reachable from where you are.
