// What people type into cards and 产出 pages, as an audit entry keeps it (1.1): redacted with the
// daemon's token-format list (apps/daemon/src/redact.rs) and cut to the first 1 KiB of JSON.
import { utf8Bytes } from "@chat/protocol";

const MASK = "<redacted>";

const RULES: [RegExp, string][] = [
  [/-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----/g, MASK],
  [/(bearer\s+)[A-Za-z0-9._~+/=-]{8,}/gi, `$1${MASK}`],
  [/((?:authorization|proxy-authorization|x-admin-secret|x-hook-secret|x-api-key|cookie|set-cookie)\s*[:=]\s*)[^\r\n,;]+/gi, `$1${MASK}`],
  [/\b(?:tc|whk|tm)_[A-Za-z0-9_-]{20,}/g, MASK],
  [/\bgh[pousr]_[A-Za-z0-9]{30,}/g, MASK],
  [/\bgithub_pat_[A-Za-z0-9_]{20,}/g, MASK],
  [/\bsk-[A-Za-z0-9_-]{20,}/g, MASK],
  [/\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g, MASK],
  [/\bxox[abprs]-[A-Za-z0-9-]{10,}/g, MASK],
  [/\bAIza[0-9A-Za-z_-]{35}/g, MASK],
  [
    /([?&](?:code|token|access_token|refresh_token|secret|sig|signature|key|api_key|apikey|password|auth|x-amz-signature|x-amz-credential|x-amz-security-token)=)[^&\s#]+/gi,
    `$1${MASK}`,
  ],
  [/([a-z][a-z0-9+.-]*:\/\/)[^/\s:@]+:[^/\s@]+@/gi, `$1${MASK}@`],
  // Invite codes (Crockford, upper case), with a digit and a letter.
  [/\b(?=[0-9A-HJKMNP-TV-Z-]*[0-9])(?=[0-9A-HJKMNP-TV-Z-]*[A-HJKMNP-TV-Z])[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}(?:-[0-9A-HJKMNP-TV-Z]{4})?\b/g, MASK],
];

/** Secret-named `key: value` pairs whose value looks like a secret (8+ characters with a digit). */
const PAIR = /("?[A-Za-z0-9_.-]*(?:token|secret|password|passwd|api_?key|access_?key|private_?key|invite_?code|client_?secret|credentials?)"?\s*[:=]\s*"?)([^"\s,;&}'`)]+)/gi;

/** Masks the token formats the daemon masks. */
export function redactText(text: string): string {
  let out = text;
  for (const [re, to] of RULES) out = out.replace(re, to);
  return out.replace(PAIR, (all, name: string, value: string) =>
    value.length >= 8 && /[0-9]/.test(value) && !value.startsWith(MASK) ? `${name}${MASK}` : all,
  );
}

/** Max UTF-8 bytes of a value in an audit entry. */
export const AUDIT_VALUE_BYTES = 1024;

/** The value as JSON text, redacted, cut to 1 KiB (on a code point boundary). */
export function auditValue(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  const text = redactText(JSON.stringify(value));
  if (utf8Bytes(text) <= AUDIT_VALUE_BYTES) return text;
  let out = "";
  let bytes = 0;
  for (const ch of text) {
    const n = utf8Bytes(ch);
    if (bytes + n > AUDIT_VALUE_BYTES - 3) break;
    out += ch;
    bytes += n;
  }
  return `${out}…`;
}
