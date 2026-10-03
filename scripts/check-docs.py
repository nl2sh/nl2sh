#!/usr/bin/env python3
"""Check bilingual page parity, navigation, links, and rendered language routes."""
import argparse
from html.parser import HTMLParser
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit
import yaml

ROOT = Path(__file__).resolve().parents[1]


def nav_pages(value):
    if isinstance(value, str):
        if value.endswith('.md'):
            yield value
    elif isinstance(value, list):
        for item in value:
            yield from nav_pages(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from nav_pages(item)


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids, self.links = set(), []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.add(attrs['id'])
        if tag == 'a' and attrs.get('href'):
            self.links.append(attrs['href'])
        if tag in ('img', 'script') and attrs.get('src'):
            self.links.append(attrs['src'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', type=Path)
    args = parser.parse_args()
    config = yaml.load((ROOT / 'mkdocs.yml').read_text(), Loader=yaml.BaseLoader)
    expected = set(nav_pages(config['nav']))
    errors = []
    sets = {lang: {p.relative_to(ROOT / f'docs/{lang}').as_posix() for p in (ROOT / f'docs/{lang}').rglob('*.md')} for lang in ('zh', 'en')}
    for lang, pages in sets.items():
        if pages != expected:
            errors.append(f'{lang}: missing={sorted(expected-pages)}, outside nav={sorted(pages-expected)}')
        for name in sorted(pages):
            path = ROOT / f'docs/{lang}' / name
            text = path.read_text()
            if any(ord(c) < 32 and c not in '\n\t' for c in text):
                errors.append(f'control character in page: {lang}/{name}')
            prose = re.sub(r'(^|\n)[ \t]*```[^\n]*\n.*?\n[ \t]*```', '', text, flags=re.S)
            if len(prose.strip()) < 100 or re.search(r'(?im)^\s*(TODO|TBD|待翻译|translation pending)\s*$', prose):
                errors.append(f'incomplete page: {lang}/{name}')
            for link in re.findall(r'\]\(([^\s)]+)', prose):
                parsed = urlsplit(link)
                if parsed.scheme or parsed.netloc or not parsed.path:
                    continue
                linkpath = unquote(parsed.path)
                target = (path.parent / linkpath).resolve()
                # Shared assets live outside locale roots.
                if not target.exists() and '/assets/' in str(target):
                    target = ROOT / 'docs/assets' / str(target).split('/assets/', 1)[1]
                if not target.exists():
                    errors.append(f'broken source link {lang}/{name}: {link}')
    for lang in ('zh', 'en'):
        for name in ('configuration', 'cli', 'tool-catalog'):
            text = (ROOT / f'docs/{lang}/reference/{name}.md').read_text()
            if '<!-- generated:start -->\n\n' not in text:
                errors.append(f'missing generated reference: {lang}/{name}')
    zh = (ROOT / 'docs/zh/changelog.md').read_text()
    en = (ROOT / 'docs/en/changelog.md').read_text()
    if re.findall(r'^## .+', zh, re.M) != re.findall(r'^## .+', en, re.M):
        errors.append('changelog version/date mismatch')
    if sum(l.startswith('- ') for l in zh.splitlines()) != sum(l.startswith('- ') for l in en.splitlines()):
        errors.append('changelog translation entry count mismatch')
    if args.site:
        site = args.site.resolve()
        html = {p.resolve(): Page(p.read_text()) for p in site.rglob('*.html')}
        for lang, prefix in [('zh', ''), ('en', 'en/')]:
            for name in expected:
                route = '' if name == 'index.md' else name[:-3] + '/'
                if route.endswith('/index/'):
                    route = route[:-6]
                path = site / prefix / route / 'index.html'
                if path not in html:
                    errors.append(f'missing language route: {prefix}{route}')
                    continue
                edit = f'https://github.com/nl2sh/nl2sh/edit/master/docs/{lang}/{name}'
                if edit not in html[path].links:
                    errors.append(f'wrong edit target: {lang}/{name}')
                other = site / ('en/' if lang == 'zh' else '') / route / 'index.html'
                local_targets = []
                for link in html[path].links:
                    u = urlsplit(link)
                    if not u.scheme and not u.netloc:
                        dest = (site / unquote(u.path).removeprefix('/nl2sh/').lstrip('/')).resolve() if u.path.startswith('/') else (path.parent / unquote(u.path)).resolve()
                        if dest.is_dir():
                            dest /= 'index.html'
                        local_targets.append(dest)
                if other not in local_targets:
                    errors.append(f'language switch loses page: {lang}/{name}')
        for path, parsed in html.items():
            for link in parsed.links:
                url = urlsplit(link)
                if url.scheme or url.netloc:
                    continue
                if url.path.startswith('/'):
                    relative = unquote(url.path).removeprefix('/nl2sh/').lstrip('/')
                    target = site / relative
                else:
                    target = (path.parent / unquote(url.path)).resolve()
                if target.is_dir():
                    target /= 'index.html'
                if not target.exists():
                    errors.append(f'broken rendered link {path.relative_to(site)}: {link}')
                elif url.fragment and target in html and unquote(url.fragment) not in html[target].ids:
                    errors.append(f'broken anchor {path.relative_to(site)}: {link}')
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'{len(expected)} pages per language: parity, navigation, and links pass' + ('; rendered routes/edit links/language switches pass' if args.site else ''))

if __name__ == '__main__':
    main()
