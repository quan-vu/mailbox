import { execFileSync, spawn } from 'node:child_process';
import path from 'node:path';

/** Controls the real local demo API (mailbox-api on 127.0.0.1:3000) for outage tests. */
const API = 'http://127.0.0.1:3000';
const BINARY = path.resolve(__dirname, '../target/debug/mailbox-api');

export async function apiIsUp(): Promise<boolean> {
  try {
    const res = await fetch(`${API}/healthz`);
    return res.ok;
  } catch {
    return false;
  }
}

async function waitFor(up: boolean) {
  for (let i = 0; i < 300; i++) {
    if ((await apiIsUp()) === up) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`mailbox-api did not become ${up ? 'available' : 'unavailable'}`);
}

export async function stopApi() {
  try {
    const pids = execFileSync('lsof', ['-ti', 'tcp:3000', '-sTCP:LISTEN']).toString().trim();
    for (const pid of pids.split('\n').filter(Boolean)) process.kill(Number(pid));
  } catch {
    // nothing is listening
  }
  await waitFor(false);
}

export async function startApi() {
  if (await apiIsUp()) return;
  const child = spawn(BINARY, [], { detached: true, stdio: 'ignore' });
  child.unref();
  await waitFor(true);
}
