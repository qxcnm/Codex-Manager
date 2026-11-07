import * as React from "react"
import { Input as InputPrimitive } from "@base-ui/react/input"

import { cn } from "@/lib/utils"

/**
 * 函数 `Input`
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
function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <InputPrimitive
      type={type}
      data-slot="input"
      className={cn(
        "h-8 w-full min-w-0 rounded-md border border-control-border bg-control-surface px-2.5 py-1 text-base text-foreground transition-[background-color,border-color,box-shadow] duration-150 outline-none file:inline-flex file:h-6 file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground enabled:hover:border-control-border-hover focus-visible:border-primary focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-primary disabled:pointer-events-none disabled:cursor-not-allowed disabled:bg-control-surface-hover disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 aria-invalid:focus-visible:outline-destructive md:text-sm",
        className
      )}
      {...props}
    />
  )
}

export { Input }
