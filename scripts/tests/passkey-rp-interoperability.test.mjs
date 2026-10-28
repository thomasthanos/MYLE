import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { verifyRegistrationResponse, verifyAuthenticationResponse } from "@simplewebauthn/server";
const result = spawnSync("cargo", ["run", "--quiet", "--locked", "--manifest-path", "backend/Cargo.toml", "-p", "myle-vault", "--example", "rp-vectors"], {
  cwd: new URL("../../", import.meta.url), encoding: "utf8", windowsHide: true, timeout: 180_000, maxBuffer: 2 * 1024 * 1024,
});
assert.equal(result.status, 0, result.error?.message ?? result.stderr);
const vectors = JSON.parse(result.stdout);
async function rejectsVerification(run) {
  try { assert.equal((await run()).verified, false); }
  catch (error) { if (error.code === "ERR_ASSERTION") throw error; }
}
for (const vector of vectors) {
  test(`independent RP accepts MYLE registration and assertion: ${vector.name}`, async () => {
    const registration = await verifyRegistrationResponse({ response: vector.registration, expectedChallenge: vector.registrationChallenge,
      expectedOrigin: vector.origin, expectedRPID: vector.rpId, requireUserVerification: true, supportedAlgorithmIDs: [-7] });
    assert.equal(registration.verified, true);
    assert.equal(registration.registrationInfo.aaguid, "1afc30cd-b888-46ee-8d17-ef0f4e5e5089");
    const base = { response:vector.authentication, expectedChallenge:vector.authenticationChallenge, expectedOrigin:vector.origin,
      expectedRPID:vector.rpId, requireUserVerification:true, credential:registration.registrationInfo.credential };
    assert.equal((await verifyAuthenticationResponse(base)).verified, true);
    await rejectsVerification(() => verifyAuthenticationResponse({ ...base, expectedChallenge: "different-challenge" }));
    await rejectsVerification(() => verifyAuthenticationResponse({ ...base, expectedOrigin: "https://evil.com" }));
    await rejectsVerification(() => verifyAuthenticationResponse({ ...base, expectedRPID: "evil.com" }));
    await rejectsVerification(() => verifyAuthenticationResponse({ ...base, response:vector.unverified }));
    const altered = structuredClone(vector.authentication);
    const signature = Buffer.from(altered.response.signature, "base64url");
    signature[signature.length - 1] ^= 1;
    altered.response.signature = signature.toString("base64url");
    await rejectsVerification(() => verifyAuthenticationResponse({ ...base, response:altered }));
  });
}
