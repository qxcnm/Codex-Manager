"use client";

import { useRef, useState, type ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { getAppErrorMessage } from "@/lib/api/transport";

export function ConnectionApplyDialog({
  t,
  label,
  target,
  icon,
  disabled,
  onApply,
  children,
}: {
  t: (value: string) => string;
  label: string;
  target: string;
  icon: ReactNode;
  disabled: boolean;
  onApply: () => Promise<unknown>;
  children: ReactNode;
}) {
  const [snapshot, setSnapshot] = useState<{
    target: string;
    content: ReactNode;
    apply: () => Promise<unknown>;
  } | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const applying = useRef(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  const confirm = async () => {
    if (!snapshot || applying.current) return;
    applying.current = true;
    setPending(true);
    setError(null);
    try {
      await snapshot.apply();
      setSnapshot(null);
    } catch (error) {
      setError(getAppErrorMessage(error));
    } finally {
      applying.current = false;
      setPending(false);
    }
  };

  return (
    <>
      <Button
        ref={triggerRef}
        type="button"
        disabled={disabled || pending}
        className="w-fit"
        onClick={() => {
          setError(null);
          setSnapshot({ target, content: children, apply: onApply });
        }}
      >
        {icon}
        {label}
      </Button>
      <Dialog
        open={snapshot !== null}
        onOpenChange={(open) => {
          if (!open && !applying.current) setSnapshot(null);
        }}
      >
        <DialogContent
          initialFocus={cancelRef}
          finalFocus={triggerRef}
          showCloseButton={!pending}
          className="flex flex-col"
        >
          <DialogHeader className="shrink-0">
            <DialogTitle>{t("应用接入配置")}</DialogTitle>
            <DialogDescription className="break-words pr-6">
              {snapshot?.target}
            </DialogDescription>
          </DialogHeader>
          <div className="grid min-h-0 gap-3 overflow-y-auto text-sm">
            {snapshot?.content}
            {error ? (
              <Alert variant="destructive">
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            ) : null}
          </div>
          <DialogFooter className="shrink-0">
            <Button
              ref={cancelRef}
              variant="outline"
              disabled={pending}
              onClick={() => setSnapshot(null)}
            >
              {t("取消")}
            </Button>
            <Button disabled={pending || disabled} onClick={() => void confirm()}>
              {pending ? <Loader2 data-icon="inline-start" className="animate-spin" /> : null}
              {t("确认应用")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
