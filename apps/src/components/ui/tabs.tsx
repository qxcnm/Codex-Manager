"use client"

import { Tabs as TabsPrimitive } from "@base-ui/react/tabs"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

/**
 * 函数 `Tabs`
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
function Tabs({
  className,
  orientation = "horizontal",
  ...props
}: TabsPrimitive.Root.Props) {
  return (
    <TabsPrimitive.Root
      data-slot="tabs"
      data-orientation={orientation}
      className={cn(
        "group/tabs flex gap-2",
        orientation === "horizontal" ? "flex-col" : "flex-row",
        className
      )}
      {...props}
    />
  )
}

const tabsListVariants = cva(
  "group/tabs-list inline-flex items-center justify-center rounded-lg p-[3px] text-muted-foreground",
  {
    variants: {
      variant: {
        default: "bg-control-surface ring-1 ring-inset ring-control-border",
        line: "gap-1 bg-transparent",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

/**
 * 函数 `TabsList`
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
function TabsList({
  className,
  variant = "default",
  ...props
}: TabsPrimitive.List.Props & VariantProps<typeof tabsListVariants>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      data-variant={variant}
      className={cn(tabsListVariants({ variant }), className)}
      {...props}
    />
  )
}

/**
 * 函数 `TabsTrigger`
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
function TabsTrigger({ className, ...props }: TabsPrimitive.Tab.Props) {
  return (
    <TabsPrimitive.Tab
      data-slot="tabs-trigger"
      className={cn(
        "relative inline-flex h-9 items-center justify-center gap-1.5 rounded-md border border-transparent px-3 py-1 text-sm font-medium whitespace-nowrap text-muted-foreground transition-[color,background-color,border-color,box-shadow] duration-150 outline-none hover:text-foreground focus-visible:border-primary focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-primary disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
        "group-data-[variant=default]/tabs-list:data-active:border-control-border group-data-[variant=default]/tabs-list:data-active:bg-control-surface-hover group-data-[variant=default]/tabs-list:data-active:text-foreground group-data-[variant=default]/tabs-list:data-active:shadow-xs",
        "group-data-[variant=line]/tabs-list:rounded-none group-data-[variant=line]/tabs-list:border-b-2 group-data-[variant=line]/tabs-list:bg-transparent group-data-[variant=line]/tabs-list:shadow-none group-data-[variant=line]/tabs-list:data-active:border-b-primary group-data-[variant=line]/tabs-list:data-active:text-primary",
        className
      )}
      {...props}
    />
  )
}

/**
 * 函数 `TabsContent`
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
function TabsContent({ className, ...props }: TabsPrimitive.Panel.Props) {
  return (
    <TabsPrimitive.Panel
      data-slot="tabs-content"
      className={cn("flex-1 text-sm outline-none w-full", className)}
      {...props}
    />
  )
}

export { Tabs, TabsList, TabsTrigger, TabsContent, tabsListVariants }
