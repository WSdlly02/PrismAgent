export function approvalMaskForAll(toolCount: number) {
  if (toolCount <= 0) {
    return 0;
  }
  const capped = Math.min(toolCount, 53);
  return 2 ** capped - 1;
}

export function approvalMaskForManual(manualApprovalMask: number) {
  return manualApprovalMask;
}
