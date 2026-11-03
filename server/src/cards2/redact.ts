// Redaction of values people type into cards, before they go into the audit log. The rules
// are the token formats of the daemon (apps/daemon/src/redact.rs): PEM private keys, bearer and
// auth headers, chat/hook/machine tokens, GitHub, OpenAI/Anthropic, AWS, Slack and Google
// keys, secret query parameters, URL user info, secret-named pairs and invite codes.

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
];
const PAIR =
  /("?[A-Za-z0-9_.-]*(?:token|secret|password|passwd|api_?key|access_?key|private_?key|invite_?code|client_?secret|credentials?)"?\s*[:=]\s*"?)([^"\s,;&}'`)]+)/gi;
const INVITE = /\b[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}(?:-[0-9A-HJKMNP-TV-Z]{4})?\b/g;

/** The text with every secret-looking value masked. */
export function redactText(text: string): string {
  let out = text;
  for (const [re, to] of RULES) out = out.replace(re, to);
  out = out.replace(PAIR, (all, name: string, value: string) => (value.length >= 8 && /[0-9]/.test(value) ? `${name}${MASK}` : all));
  out = out.replace(INVITE, (code) => (/[0-9]/.test(code) && /[A-Z]/.test(code) ? MASK : code));
  return out;
}

/** A JSON value as audit text: at most `max` characters of its JSON, redacted. */
export function auditValue(value: unknown, max = 1024): string | undefined {
  if (value === undefined) return undefined;
  const json = JSON.stringify(value);
  return redactText(json.length > max ? json.slice(0, max) : json);
}
