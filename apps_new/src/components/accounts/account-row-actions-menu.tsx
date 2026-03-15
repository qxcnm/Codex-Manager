"use client";

import { useState } from "react";
import { ExternalLink, Trash2 } from "lucide-react";
import { AccountConfirmDialog } from "@/components/accounts/account-confirm-dialog";
import { Button } from "@/components/ui/button";
import type { Account } from "@/types";

interface AccountRowActionsMenuProps {
  account: Account;
  onOpenDetails: (account: Account) => void;
  onDelete: (account: Account) => void;
}

export function AccountRowActionsMenu({
  account,
  onOpenDetails,
  onDelete,
}: AccountRowActionsMenuProps) {
  const [confirmOpen, setConfirmOpen] = useState(false);

  return (
    <>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
        onClick={() => onOpenDetails(account)}
        title="详情与日志"
        aria-label="详情与日志"
      >
        <ExternalLink className="h-4 w-4" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground transition-colors hover:text-destructive"
        onClick={() => setConfirmOpen(true)}
        title="删除账号"
        aria-label="删除账号"
      >
        <Trash2 className="h-4 w-4" />
      </Button>
      <AccountConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={`确定删除账号 ${account.name} 吗？`}
        description="删除后不可恢复。"
        confirmLabel="确认删除"
        onConfirm={() => {
          setConfirmOpen(false);
          onDelete(account);
        }}
      />
    </>
  );
}
