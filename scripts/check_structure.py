#!/usr/bin/env python3
"""Static structural checks only. Does not compile Rust or verify runtime behavior."""
from pathlib import Path
import tomllib
root = Path(__file__).resolve().parents[1]
required = [
    'Cargo.toml','AGENTS.md','CLAUDE.md','README.md','LICENSE',
    'apps/mailbox-ui/src/main.rs','apps/mailbox-ui/assets/main.css',
    'apps/mailbox-ui/Dioxus.toml', 'apps/mailbox-api/src/main.rs',
    'crates/mailbox-contracts/src/lib.rs','crates/mailbox-core/src/lib.rs',
    'docs/06-delivery/TRACKING.md','docs/03-ui/SCREENS.md',
    'migrations/sqlite/0001_init.sql','migrations/postgres/0001_init.sql',
]
required.extend(f'docs/03-ui/references/{i:02d}-{name}.png' for i,name in [
    (1,'unified-inbox'),(2,'connect-account'),(3,'thread'),(4,'compose'),(5,'settings')
])
for rel in required:
    p = root / rel
    assert p.exists() and p.stat().st_size > 0, f'Missing or empty: {rel}'
for cfg in ['Cargo.toml', 'apps/mailbox-ui/Cargo.toml', 'apps/mailbox-api/Cargo.toml', 'apps/mailbox-ui/Dioxus.toml']:
    with (root/cfg).open('rb') as f: tomllib.load(f)
print(f'PASS: {len(required)} key files present; Cargo/Dioxus TOML parses. No compilation performed.')
