import type { Lease } from "../api/types";

const RENEW_LEASE_WITHIN_SECONDS = 5;

export function shouldRenewLease(
  lease: Lease | null,
  nowSeconds = Math.floor(Date.now() / 1000),
) {
  return !lease || lease.expires_at - nowSeconds <= RENEW_LEASE_WITHIN_SECONDS;
}
