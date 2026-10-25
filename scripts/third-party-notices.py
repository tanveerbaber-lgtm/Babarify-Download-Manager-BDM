"""Generates THIRD_PARTY_NOTICES.txt: the license text of every open-source
component shipped in BDM (Rust crates in the Windows build and the npm
packages in the UI bundle). Run before a release:

    python scripts/third-party-notices.py
"""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_NOTICES.txt"
LICENSE_NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT")


def license_files(folder: Path) -> list[Path]:
    if not folder.is_dir():
        return []
    return sorted(p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(LICENSE_NAMES))


def rust_crates():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"),
         "--filter-platform", "x86_64-pc-windows-msvc"],
        capture_output=True, text=True, encoding="utf-8", check=True).stdout)
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    pkgs = {p["id"]: p for p in meta["packages"]}
    root_id = meta["resolve"]["root"]
    seen, stack = set(), [root_id]
    while stack:
        cur = stack.pop()
        if cur in seen:
            continue
        seen.add(cur)
        for dep in nodes[cur]["deps"]:
            if any(k["kind"] in (None, "normal") for k in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    seen.discard(root_id)
    for pid in sorted(seen, key=lambda i: (pkgs[i]["name"], pkgs[i]["version"])):
        p = pkgs[pid]
        yield p["name"], p["version"], p.get("license") or "see files", Path(p["manifest_path"]).parent, p.get("repository")


def npm_packages():
    tree = json.loads(subprocess.run("npm ls --omit=dev --all --json", cwd=ROOT, capture_output=True, text=True,
                                     encoding="utf-8", shell=True).stdout)
    seen = {}

    def walk(deps):
        for name, info in (deps or {}).items():
            if name not in seen:
                folder = ROOT / "node_modules" / name
                data = json.loads((folder / "package.json").read_text(encoding="utf-8"))
                lic = data.get("license") if isinstance(data.get("license"), str) else "see files"
                repo = data.get("repository")
                repo = repo.get("url") if isinstance(repo, dict) else repo
                seen[name] = (name, data.get("version", ""), lic, folder, repo)
            walk(info.get("dependencies"))

    walk(tree.get("dependencies"))
    return [seen[k] for k in sorted(seen)]


def main():
    parts = [
        "Babarify Download Manager — Third-Party Notices",
        "",
        "Babarify Download Manager includes the open-source components listed below.",
        "Each is used under its own license, reproduced here. yt-dlp, FFmpeg and Deno are",
        "not included: BDM downloads them from their official releases only if you choose",
        "to install them (see THIRD_PARTY.md).",
        "",
    ]
    missing = []
    count = 0
    for section, items in (("Rust crates", rust_crates()), ("JavaScript packages", npm_packages())):
        parts += ["=" * 78, section, "=" * 78, ""]
        texts_seen = {}
        for name, version, lic, folder, repo in items:
            count += 1
            parts.append("-" * 78)
            parts.append(f"{name} {version} — {lic}" + (f"\n{repo}" if repo else ""))
            files = license_files(folder)
            if not files:
                missing.append(f"{name} {version}")
                parts.append(f"(No license file in the package; licensed under {lic}.)")
            for f in files:
                text = f.read_text(encoding="utf-8", errors="replace").strip()
                # Identical texts (e.g. the same Apache-2.0 file) are printed once and referenced after.
                key = hash(text)
                if key in texts_seen and len(text) > 2000:
                    parts.append(f"[{f.name}: same text as {texts_seen[key]} above]")
                else:
                    texts_seen.setdefault(key, f"{name} {version} ({f.name})")
                    parts += [f"[{f.name}]", text]
            parts.append("")
    OUT.write_text("\n".join(parts) + "\n", encoding="utf-8")
    print(f"{count} components written to {OUT.name}; {len(missing)} without a license file")
    for m in missing:
        print("  no license file:", m)


if __name__ == "__main__":
    main()
