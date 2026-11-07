import * as React from "react"

import { cn } from "@/lib/utils"

/**
 * 函数 `Textarea`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 *
 * # 返回
 * 返回函数执行结果
 */
function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "flex min-h-16 w-full rounded-md border border-control-border bg-control-surface px-2.5 py-2 text-base text-foreground transition-[background-color,border-color,box-shadow] duration-150 outline-none placeholder:text-muted-foreground enabled:hover:border-control-border-hover focus-visible:border-primary focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:bg-control-surface-hover disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 aria-invalid:focus-visible:outline-destructive md:text-sm",
        className
      )}
      {...props}
    />
  )
}

export { Textarea }
