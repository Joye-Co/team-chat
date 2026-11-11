// Account sign-in beyond invite and email codes: an optional password, and sign-in by a one-time
// code that a signed-in device shows or approves (扫码登录). See docs/protocol.md, "Password
// sign-in" and "Code sign-in".
import { z } from "zod";
import { Id, Millis } from "./common.ts";
import { EmailAddress } from "./settings.ts";

const codePoints = (s: string) => [...s].length;

/** A new password: 8 to 128 characters (code points), not only spaces. */
export const Password = z
  .string()
  .refine((s) => codePoints(s) >= 8, "at least 8 characters")
  .refine((s) => codePoints(s) <= 128, "at most 128 characters")
  .refine((s) => s.trim().length > 0, "not only spaces");
export type Password = z.infer<typeof Password>;

/** A password as typed at sign in or as the current one: 1 to 128 characters. */
const GivenPassword = z
  .string()
  .min(1)
  .refine((s) => codePoints(s) <= 128, "at most 128 characters");

const DeviceName = z
  .string()
  .min(1)
  .refine((s) => codePoints(s) <= 128, "at most 128 characters");

/** `GET`, `PUT /v1/me/password` and `POST /v1/me/password/remove`. */
export const MyPasswordResponse = z.object({ set: z.boolean(), updatedAt: Millis.nullable() });
export type MyPasswordResponse = z.infer<typeof MyPasswordResponse>;

/** `PUT /v1/me/password`: sets or changes my password. */
export const SetPasswordRequest = z.object({
  password: Password,
  /** Needed when a password is set, unless this token signed in within the last 10 minutes. */
  currentPassword: GivenPassword.optional(),
});
export type SetPasswordRequest = z.infer<typeof SetPasswordRequest>;

/** `POST /v1/me/password/remove`: same rule for `currentPassword` as setting one. */
export const RemovePasswordRequest = z.object({ currentPassword: GivenPassword.optional() });
export type RemovePasswordRequest = z.infer<typeof RemovePasswordRequest>;

/** `POST /v1/auth/password/sign-in`; answers `RedeemResponse`. */
export const PasswordSignInRequest = z.object({
  email: EmailAddress,
  password: GivenPassword,
  deviceName: DeviceName,
});
export type PasswordSignInRequest = z.infer<typeof PasswordSignInRequest>;

/**
 * A one-time sign-in code as typed or scanned: 8 Crockford characters shown `XXXX-XXXX`, read
 * like an invite code (case, dashes and spaces ignored, I/L as 1, O as 0).
 */
export const SignInCode = z.string().min(1).max(32);

/** `POST /v1/me/qr/offer`: a code that signs a new device in to my account. */
export const QrOfferResponse = z.object({ id: Id, code: z.string(), expiresAt: Millis });
export type QrOfferResponse = z.infer<typeof QrOfferResponse>;

/** `GET /v1/me/qr/offers/:id`: whether a device used my code. */
export const QrOfferStatus = z.object({
  state: z.enum(["waiting", "used", "expired"]),
  /** The device that signed in with it, when `used`. */
  deviceName: z.string().max(128).optional(),
});
export type QrOfferStatus = z.infer<typeof QrOfferStatus>;

/** `POST /v1/auth/qr/claim`: sign in with a code a signed-in device shows; answers `RedeemResponse`. */
export const QrClaimRequest = z.object({ code: SignInCode, deviceName: DeviceName });
export type QrClaimRequest = z.infer<typeof QrClaimRequest>;

/** `POST /v1/auth/qr/request`: a new device asks a signed-in one to let it in. */
export const QrRequestRequest = z.object({ deviceName: DeviceName });
export type QrRequestRequest = z.infer<typeof QrRequestRequest>;

/** The code to show, and the secret only this device knows for polling. */
export const QrRequestResponse = z.object({ id: Id, code: z.string(), secret: z.string(), expiresAt: Millis });
export type QrRequestResponse = z.infer<typeof QrRequestResponse>;

/** `POST /v1/auth/qr/poll`; answers `QrPollResponse` (in index.ts, it carries a `Member`). */
export const QrPollRequest = z.object({ id: Id, secret: z.string().min(1).max(128) });
export type QrPollRequest = z.infer<typeof QrPollRequest>;

/** `POST /v1/me/qr/peek` and `POST /v1/me/qr/approve`: the code another device shows. */
export const QrCodeRequest = z.object({ code: SignInCode });
export type QrCodeRequest = z.infer<typeof QrCodeRequest>;

/** `POST /v1/me/qr/peek`: which device asks, before I approve it. */
export const QrPeekResponse = z.object({ deviceName: z.string().max(128), expiresAt: Millis });
export type QrPeekResponse = z.infer<typeof QrPeekResponse>;
