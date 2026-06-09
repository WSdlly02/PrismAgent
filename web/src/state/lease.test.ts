import { describe, expect, it } from "vitest";
import { shouldRenewLease } from "./lease";
import type { Lease } from "../api/types";

const lease: Lease = {
  workspace_uuid: "workspace-1",
  client_id: "client-1",
  lease_token: "lease-1",
  expires_at: 100,
};

describe("lease renewal", () => {
  it("renews missing leases", () => {
    expect(shouldRenewLease(null, 90)).toBe(true);
  });

  it("renews leases before they expire", () => {
    expect(shouldRenewLease(lease, 96)).toBe(true);
  });

  it("keeps leases with enough remaining time", () => {
    expect(shouldRenewLease(lease, 80)).toBe(false);
  });
});
