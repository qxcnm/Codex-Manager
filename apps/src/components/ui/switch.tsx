"use client"

import { Switch as SwitchPrimitive } from "@base-ui/react/switch"

import { cn } from "@/lib/utils"

/**
 * 函数 `Switch`
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
function Switch({
  className,
  size = "default",
  ...props
}: SwitchPrimitive.Root.Props & {
  size?: "sm" | "default"
}) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      data-size={size}
      className={cn(
        "peer group/switch relative inline-flex shrink-0 items-center rounded-full border transition-[background-color,border-color,box-shadow] duration-150 outline-none after:absolute after:-inset-x-3 after:-inset-y-2 focus-visible:border-primary focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-primary aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 aria-invalid:focus-visible:outline-destructive data-[size=default]:h-[18.4px] data-[size=default]:w-[32px] data-[size=sm]:h-[14px] data-[size=sm]:w-[24px] dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 data-checked:border-primary/40 data-checked:bg-primary data-unchecked:border-foreground/20 data-unchecked:bg-foreground/18 dark:data-unchecked:border-foreground/30 dark:data-unchecked:bg-foreground/20 data-disabled:cursor-not-allowed data-disabled:opacity-50",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block rounded-full bg-background shadow-sm ring-0 transition-transform group-data-[size=default]/switch:size-4 group-data-[size=sm]/switch:size-3 group-data-[size=default]/switch:data-checked:translate-x-[calc(100%-2px)] group-data-[size=sm]/switch:data-checked:translate-x-[calc(100%-2px)] data-checked:bg-primary-foreground group-data-[size=default]/switch:data-unchecked:translate-x-0 group-data-[size=sm]/switch:data-unchecked:translate-x-0 data-unchecked:bg-white data-unchecked:ring-1 data-unchecked:ring-foreground/20 dark:data-unchecked:bg-foreground dark:data-unchecked:ring-background/20"
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
