"""Update the workspace version and every derived manifest that restates it."""

import glob
import pathlib
import re
import sys
import tomllib

# A release is either stable or a nightly of the stable base it is warming up:
# `X.Y.Z` or `X.Y.Z-nightly.<date>.<run>`. The trailing pair orders nightlies within a base.
SEMANTIC_VERSION = (
    r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
    r'(?:-nightly\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*))?'
)

ARCH_RECIPE = pathlib.Path('packaging/aur/PKGBUILD')


def arch_pkgver(version):
    """Render a release version the way packaging/linux/package.py names an Arch artifact."""
    return version.replace('-', 'pre.', 1).replace('-', '.')


def release_order(version):
    """Sort key for release versions; a stable release sorts above its own nightlies."""
    match = re.fullmatch(SEMANTIC_VERSION, version)
    if match is None:
        raise ValueError(f'Not a Bytwina release version: {version}')
    major, minor, patch, date, build = match.groups()
    if date is None:
        return int(major), int(minor), int(patch), 1, 0, 0
    return int(major), int(minor), int(patch), 0, int(date), int(build)


def literal(pattern, source, label):
    match = re.search(pattern, source)
    if match is None:
        raise ValueError(f'Cannot locate {label}')
    return match['value']


def rewrite(path, pattern, value, label):
    """Replace one quoted version literal. Manifests this tree does not carry are skipped."""
    if not path.is_file():
        return
    source, count = re.subn(pattern, lambda match: match[1] + value + match[2], path.read_text())
    if count != 1:
        raise ValueError(f'Cannot locate {label}')
    path.write_text(source)


def check_arch_recipe(version):
    """Verify the binary recipe instead of rewriting it: pkgver names a published asset."""
    if not ARCH_RECIPE.is_file():
        return
    source = ARCH_RECIPE.read_text()
    pkgver = literal(r"(?m)^pkgver=(?P<value>[^\s'\"]+)$", source, 'Arch pkgver')
    pkgrel = literal(r'(?m)^pkgrel=(?P<value>\d+)$', source, 'Arch pkgrel')
    url = literal(r"(?m)^source_x86_64=\('(?P<value>[^']+)'\)", source, 'Arch source URL')
    literal(r"(?m)^sha256sums_x86_64=\('(?P<value>[0-9a-f]{64})'\)", source, 'Arch checksum')
    # pacman names a package from pkgver and pkgrel, so the asset must carry both verbatim.
    pin = re.search(
        r'/releases/download/(?P<tag>v[^/]+)/bytwina-(?P=tag)-Linux-[A-Za-z0-9.]+-'
        r'bytwina-(?P<pkgver>[A-Za-z0-9._+]+)-(?P<pkgrel>\d+)-[A-Za-z0-9_]+\.pkg\.tar\.zst$',
        url,
    )
    if pin is None:
        raise ValueError(f'Arch recipe does not pin an asset named from its release tag: {url}')
    if (pin['pkgver'], pin['pkgrel']) != (pkgver, pkgrel):
        raise ValueError(
            f'Arch recipe declares pkgver {pkgver}-{pkgrel} but installs '
            f'{pin["pkgver"]}-{pin["pkgrel"]}; update pkgver with the asset URL and checksum.'
        )
    pinned = pin['tag'].removeprefix('v')
    if arch_pkgver(pinned) != pkgver:
        raise ValueError(f'Arch recipe pins release v{pinned} under pkgver {pkgver}')
    if release_order(pinned) > release_order(version):
        raise ValueError(f'Arch recipe pins v{pinned}, newer than the {version} being released')


def update(version):
    if not re.fullmatch(SEMANTIC_VERSION, version):
        raise ValueError('Expected a stable or nightly semantic version')
    manifest = pathlib.Path('Cargo.toml')
    source = manifest.read_text()
    workspace = tomllib.loads(source)['workspace']
    previous = workspace['package']['version']
    names = set()
    for pattern in workspace['members']:
        for path in glob.glob(f'{pattern}/Cargo.toml'):
            package = tomllib.loads(pathlib.Path(path).read_text())['package']
            if package.get('version') == {'workspace': True}:
                names.add(package['name'])
    assert 'bytwina' in names, 'Desktop must inherit the workspace version'
    updated, count = re.subn(
        r'(\[workspace\.package\]\s*\nversion\s*=\s*")[^"]+("\s*\n)',
        lambda match: match[1] + version + match[2], source, count=1,
    )
    assert count == 1, 'Cannot locate workspace version'
    manifest.write_text(updated)
    for path in map(pathlib.Path, ['Cargo.lock', 'fuzz/Cargo.lock']):
        sections = path.read_text().split('[[package]]')
        for index in range(1, len(sections)):
            section = sections[index]
            package = tomllib.loads(section)
            if package['name'] in names and 'source' not in package:
                assert package['version'] == previous, 'Unexpected local lockfile version'
                section = re.sub(r'(?m)^version = "[^"]+"$', f'version = "{version}"', section, count=1)
            # Cargo disambiguates duplicate names as "name version" in dependency lists.
            for name in names:
                section = section.replace(f'"{name} {previous}"', f'"{name} {version}"')
            sections[index] = section
        path.write_text('[[package]]'.join(sections))
    stable = version.split('-')[0]
    plist = pathlib.Path('packaging/macos/Info.plist')
    rewrite(plist, r'(<key>CFBundleShortVersionString</key><string>)[^<]+(</string>)',
            stable, 'Mac bundle version')
    if plist.is_file() and '<key>CFBundleVersion</key>' in plist.read_text():
        rewrite(plist, r'(<key>CFBundleVersion</key><string>)[^<]+(</string>)',
                stable, 'Mac build version')
    rewrite(pathlib.Path('packaging/windows/installer.nsi'),
            r'(?m)^([ \t]*!define VERSION ")[^"]+("[ \t]*)$', version, 'Windows installer version')
    rewrite(pathlib.Path('Casks/bytwina.rb'),
            r'(?m)^([ \t]*version ")[^"]+("[ \t]*)$', version, 'Homebrew cask version')
    check_arch_recipe(version)


if __name__ == '__main__':
    update(sys.argv[1])
