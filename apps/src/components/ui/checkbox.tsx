"use client"

import { Checkbox as CheckboxPrimitive } from "@base-ui/react/checkbox"

import { cn } from "@/lib/utils"
import { CheckIcon, MinusIcon } from "lucide-react"

/**
 * 函数 `Checkbox`
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
function Checkbox({ className, ...props }: CheckboxPrimitive.Root.Props) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      className={cn(
        "peer group/checkbox relative flex size-4.5 shrink-0 items-center justify-center rounded-[4px] border-[1.5px] border-checkbox-border bg-checkbox-surface text-primary-foreground transition-[background-color,border-color,box-shadow] outline-none after:absolute after:-inset-x-3 after:-inset-y-2",
        "not-data-disabled:cursor-pointer not-data-disabled:hover:border-primary not-data-disabled:hover:ring-3 not-data-disabled:hover:ring-primary/15 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-primary",
        "group-has-disabled/field:opacity-50 data-disabled:cursor-not-allowed data-disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20",
        "data-checked:border-primary data-checked:bg-primary data-indeterminate:border-primary data-indeterminate:bg-primary",
        className
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator
        data-slot="checkbox-indicator"
        className="grid place-content-center text-current transition-none [&>svg]:size-3.5"
      >
        <CheckIcon className="group-data-indeterminate/checkbox:hidden" />
        <MinusIcon className="hidden group-data-indeterminate/checkbox:block" />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  )
}

export { Checkbox }
