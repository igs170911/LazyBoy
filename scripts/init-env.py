#!/usr/bin/env python3
"""Initialize new installs only. Existing keys and vault data are preserved."""
from pathlib import Path
import secrets

path=Path('.env')
if path.exists():
    print('.env already exists; existing credentials were preserved.')
else:
    values={key:secrets.token_hex(32) for key in ('LAZYBOY_APP_TOKEN','SANDBOX_SUPERVISOR_TOKEN','LAZYBOY_VAULT_KEY','POSTGRES_PASSWORD')}
    text=Path('.env.example').read_text()
    lines=[]
    for line in text.splitlines():
        key=line.split('=',1)[0]
        if key in values:line=f'{key}={values[key]}'
        elif key=='DATABASE_URL':line=f"DATABASE_URL=postgres://lazyboy:{values['POSTGRES_PASSWORD']}@127.0.0.1:5434/lazyboy"
        lines.append(line)
    with path.open('x') as out:
        path.chmod(0o600)
        out.write('\n'.join(lines)+'\n')
    print('Created .env with independent app, supervisor, vault and database secrets. Set your model API key next.')
