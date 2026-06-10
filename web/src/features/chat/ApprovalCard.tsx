import type { PendingApproval } from "../../api/types";

type ApprovalCardProps = {
  request: PendingApproval;
  onApprove: () => void;
  onDeny: () => void;
};

export function ApprovalCard({ request, onApprove, onDeny }: ApprovalCardProps) {
  return (
    <div className="approval-card">
      <div>
        <strong>Approval required</strong>
        <p>{request.description}</p>
      </div>
      <div className="approval-actions">
        <button className="secondary-button" onClick={onDeny} type="button">
          Deny
        </button>
        <button className="primary-button" onClick={onApprove} type="button">
          Approve
        </button>
      </div>
    </div>
  );
}