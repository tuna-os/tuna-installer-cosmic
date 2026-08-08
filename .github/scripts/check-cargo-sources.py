#!/usr/bin/env python3
"""Fail if flatpak/cargo-sources.json has drifted from Cargo.lock.

Why this exists
---------------
flatpak-builder runs `cargo --offline build`, so every package in Cargo.lock
must already be vendored by flatpak/cargo-sources.json. That file is generated
by flatpak-cargo-generator.py (`just cargo-sources`) and nothing regenerates it
automatically. Renovate automerges lockfile bumps, so the two files drift
silently and the only thing that notices is the Flatpak build — which runs on
push to main, after the merge. That is how main stayed red for 14 consecutive
runs (2026-08-01 .. 2026-08-08).

This is a *structural* comparison of the two committed files. It deliberately
does not regenerate anything:

  * no network, no ~11 git clones, runs in under a second;
  * the generator is fetched from flatpak-builder-tools `master`, i.e. an
    unpinned moving target, so a regenerate-and-diff gate would go red on
    upstream's schedule rather than on ours. A gate that fails for reasons the
    PR author did not cause is a gate people learn to ignore.

It asserts exactly the property that breaks the offline build: nothing in the
lockfile is unvendored, and every git remote has the `replace-with` stanza
cargo needs to resolve it without network.

Usage: check-cargo-sources.py [--lock Cargo.lock] [--sources flatpak/cargo-sources.json]
       check-cargo-sources.py --selftest   # assert the check still rejects drift
"""

from __future__ import annotations

import argparse
import json
import shlex
import sys
import tomllib
from pathlib import Path
from urllib.parse import urlparse

# Must match flatpak-cargo-generator.py.
CARGO_HOME = "cargo"
CARGO_CRATES = f"{CARGO_HOME}/vendor"
VENDORED_SOURCES = "vendored-sources"
GIT_CACHE = "flatpak-cargo/git"
COMMIT_LEN = 7

FIX = "run `just cargo-sources` and commit flatpak/cargo-sources.json"


def canonical_url(url: str) -> str:
    """Cargo's canonical URL, as flatpak-cargo-generator.py computes it."""
    url = url.replace("git+https://", "https://")
    u = urlparse(url)
    path = u.path.rstrip("/")
    scheme, netloc = u.scheme, u.netloc
    if netloc == "github.com":
        scheme = "https"
        path = path.lower()
    if path.endswith(".git"):
        path = path[: -len(".git")]
    return f"{scheme}://{netloc}{path}"


def git_repo_name(git_url: str, commit: str) -> str:
    return f"{canonical_url(git_url).split('/')[-1]}-{commit[:COMMIT_LEN]}"


class Sources:
    """Indexed view of flatpak/cargo-sources.json."""

    def __init__(self, entries: list[dict]) -> None:
        self.archives: dict[str, dict] = {}
        self.git_repos: dict[str, dict] = {}
        self.inline: dict[tuple[str, str], dict] = {}
        self.copied_into: dict[str, str] = {}

        for entry in entries:
            kind = entry.get("type")
            dest = entry.get("dest", "")
            if kind == "archive":
                self.archives[dest] = entry
                # `--git-tarballs` mode emits git checkouts as archives too.
                if dest.startswith(f"{GIT_CACHE}/"):
                    self.git_repos[dest] = entry
            elif kind == "git":
                self.git_repos[dest] = entry
            elif kind == "inline":
                self.inline[(dest, entry.get("dest-filename", ""))] = entry
            elif kind == "shell":
                for command in entry.get("commands", []):
                    self._index_copy(command)

        self.config = self._load_config()

    def _index_copy(self, command: str) -> None:
        """Record `cp -r --reflink=auto "<src>" "cargo/vendor/<name>"`."""
        words = shlex.split(command)
        if not words or words[0] != "cp" or len(words) < 3:
            return
        src, dst = words[-2], words[-1]
        self.copied_into[dst] = src

    def _load_config(self) -> dict:
        """Parse the inline cargo config the generator appends last."""
        for filename in ("config.toml", "config"):
            entry = self.inline.get((CARGO_HOME, filename))
            if entry is not None:
                return tomllib.loads(entry.get("contents", "")).get("source", {})
        return {}

    def has_config(self) -> bool:
        return bool(self.config)

    def vendored(self, dest: str) -> bool:
        """True if `dest` is populated as a vendor dir with a checksum file."""
        populated = dest in self.archives or dest in self.copied_into
        return populated and (dest, ".cargo-checksum.json") in self.inline


def summarize(names: list[str], limit: int = 6) -> str:
    shown = ", ".join(names[:limit])
    return shown if len(names) <= limit else f"{shown} (+{len(names) - limit} more)"


def check(lock: dict, sources: Sources) -> list[str]:
    problems: list[str] = []
    expected_vendor_dirs: set[str] = set()
    # A single git remote backs up to ~18 packages here (libcosmic). Group the
    # per-remote problems so a moved commit is one line, not 36.
    missing_checkout: dict[str, tuple[str, str, list[str]]] = {}
    wrong_commit: dict[str, tuple[str, str, list[str]]] = {}
    stale_copy: dict[tuple[str, str], list[str]] = {}
    missing_stanza: dict[str, list[str]] = {}
    bad_stanza: dict[str, list[str]] = {}

    if not sources.has_config():
        problems.append(
            f"the vendor config is missing: no inline `{CARGO_HOME}/config.toml` "
            "with a [source] table. `cargo --offline build` cannot resolve "
            "anything without it."
        )
    elif sources.config.get("crates-io", {}).get("replace-with") != VENDORED_SOURCES:
        problems.append(
            f"[source.crates-io] does not `replace-with = \"{VENDORED_SOURCES}\"`; "
            "cargo would go to the network for every crates.io dependency."
        )

    for package in lock.get("package", []):
        name, version = package["name"], package["version"]
        source = package.get("source")
        if source is None:
            continue  # the workspace crate itself

        if not source.startswith("git+"):
            dest = f"{CARGO_CRATES}/{name}-{version}"
            expected_vendor_dirs.add(dest)
            if not sources.vendored(dest):
                problems.append(f"crates.io package not vendored: {name} {version}")
                continue
            checksum = package.get("checksum")
            got = sources.archives.get(dest, {}).get("sha256")
            if checksum and got and got != checksum:
                problems.append(
                    f"checksum mismatch for {name} {version}: Cargo.lock says "
                    f"{checksum}, cargo-sources.json says {got}"
                )
            continue

        commit = urlparse(source).fragment
        if not commit:
            problems.append(f"git package {name} {version} has no commit: {source}")
            continue
        repo = canonical_url(source)
        dest = f"{CARGO_CRATES}/{name}"
        expected_vendor_dirs.add(dest)

        if not sources.vendored(dest):
            problems.append(
                f"git package not vendored: {name} {version} from {repo}@{commit[:COMMIT_LEN]}"
            )
        if (dest, "Cargo.toml") not in sources.inline:
            problems.append(f"git package {name} has no vendored Cargo.toml")

        checkout = f"{GIT_CACHE}/{git_repo_name(repo, commit)}"
        entry = sources.git_repos.get(checkout)
        if entry is None:
            missing_checkout.setdefault(checkout, (repo, commit, []))[2].append(name)
        elif entry.get("type") == "git" and entry.get("commit") != commit:
            wrong_commit.setdefault(
                checkout, (entry.get("commit", "?"), commit, [])
            )[2].append(name)

        copied_from = sources.copied_into.get(dest)
        if copied_from is not None and not copied_from.startswith(f"{checkout}/"):
            have = copied_from.split("/")[2] if "/" in copied_from else copied_from
            stale_copy.setdefault((repo, have), []).append(name)

        if sources.has_config():
            stanza = sources.config.get(repo)
            if stanza is None:
                missing_stanza.setdefault(repo, []).append(name)
            elif stanza.get("replace-with") != VENDORED_SOURCES:
                bad_stanza.setdefault(repo, []).append(name)

    for checkout, (repo, commit, names) in missing_checkout.items():
        problems.append(
            f"git checkout missing: no source with dest {checkout} for {repo} @ "
            f"{commit} — needed by {len(names)} package(s): {summarize(names)}"
        )
    for checkout, (have, want, names) in wrong_commit.items():
        problems.append(
            f"git checkout {checkout} is at the wrong commit: cargo-sources.json has "
            f"{have}, Cargo.lock wants {want} — affects {summarize(names)}"
        )
    for (repo, have), names in stale_copy.items():
        problems.append(
            f"{len(names)} package(s) are vendored from a stale checkout of {repo} "
            f"({have}): {summarize(names)}"
        )
    for repo, names in missing_stanza.items():
        problems.append(
            f'git remote has no [source."{repo}"] stanza; `cargo --offline build` '
            f"fails with \"can't checkout from '{repo}'\" — needed by "
            f"{summarize(names)}"
        )
    for repo, names in bad_stanza.items():
        problems.append(
            f'[source."{repo}"] does not `replace-with = "{VENDORED_SOURCES}"` — '
            f"affects {summarize(names)}"
        )

    stale = sorted(
        dest
        for dest in set(sources.archives) | set(sources.copied_into)
        if dest.startswith(f"{CARGO_CRATES}/") and dest not in expected_vendor_dirs
    )
    problems.extend(f"vendored but not in Cargo.lock: {dest}" for dest in stale)

    return problems


def selftest(lock: dict, entries: list[dict]) -> int:
    """Guard the guard: drop one vendored crate and require a failure.

    Without this, a refactor that quietly stops finding packages would leave a
    check that always passes — indistinguishable from a check that works.
    """
    victim = next(
        (e for e in entries if e.get("dest", "").startswith(f"{CARGO_CRATES}/")), None
    )
    if victim is None:
        print("selftest: no vendored crate to remove", file=sys.stderr)
        return 1

    dest = victim["dest"]
    desynced = [e for e in entries if e.get("dest") != dest]
    problems = check(lock, Sources(desynced))
    if not problems:
        print(
            f"SELFTEST BROKEN: the check accepted a cargo-sources.json with {dest} "
            "removed. It is no longer gating anything.",
            file=sys.stderr,
        )
        return 1

    print(f"ok: removing {dest} was rejected -> {problems[0]}")
    return 0


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, default=repo_root / "Cargo.lock")
    parser.add_argument(
        "--sources", type=Path, default=repo_root / "flatpak" / "cargo-sources.json"
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="desync a copy of the sources in memory and require the check to fail",
    )
    args = parser.parse_args()

    with args.lock.open("rb") as f:
        lock = tomllib.load(f)
    with args.sources.open("rb") as f:
        entries = json.load(f)
    if not isinstance(entries, list):
        print(f"{args.sources}: expected a JSON list of flatpak sources", file=sys.stderr)
        return 1

    if args.selftest:
        return selftest(lock, entries)

    problems = check(lock, Sources(entries))
    packages = len(lock.get("package", []))

    if problems:
        print(
            f"{args.sources} is out of sync with {args.lock} "
            f"({len(problems)} problem(s)):",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            f"\nflatpak-builder runs `cargo --offline build`, so this would fail the "
            f"Flatpak build on main.\nTo fix: {FIX}.",
            file=sys.stderr,
        )
        return 1

    print(
        f"flatpak/cargo-sources.json is in sync with Cargo.lock "
        f"({packages} packages vendored)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
