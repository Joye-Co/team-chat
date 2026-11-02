// ULIDs: 48-bit millisecond time + 80 random bits, Crockford base32, 26 characters.
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

let lastTime = -1;
let lastRandom: Uint8Array = new Uint8Array(10);

export function ulid(now: number = Date.now()): string {
  let random: Uint8Array;
  if (now <= lastTime) {
    // Same millisecond (or clock went back): increment the random part so ids stay sorted.
    now = lastTime;
    random = lastRandom.slice();
    for (let i = 9; i >= 0; i--) {
      if (random[i] === 255) {
        random[i] = 0;
      } else {
        random[i]++;
        break;
      }
    }
  } else {
    random = crypto.getRandomValues(new Uint8Array(10));
  }
  lastTime = now;
  lastRandom = random;
  let time = "";
  let t = now;
  for (let i = 0; i < 10; i++) {
    time = CROCKFORD[t % 32] + time;
    t = Math.floor(t / 32);
  }
  // 80 bits -> 16 base32 characters.
  let bits = 0n;
  for (const b of random) bits = (bits << 8n) | BigInt(b);
  let rand = "";
  for (let i = 0; i < 16; i++) {
    rand = CROCKFORD[Number(bits & 31n)] + rand;
    bits >>= 5n;
  }
  return time + rand;
}

export { CROCKFORD };
