"use client";

import {
  forwardRef,
  type AnchorHTMLAttributes,
  type MouseEvent,
} from "react";
import { useAppStore } from "@/lib/store/useAppStore";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";

export interface ShellLinkProps
  extends Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  href: string;
}

/**
 * OpenRuntime 内部页面统一跳转入口。
 * 保留标准 href 和新标签页能力；普通左键点击则交给常驻页面壳切换，
 * 避免静态导出页面整页重载后被壳状态重置回首页。
 */
export const ShellLink = forwardRef<HTMLAnchorElement, ShellLinkProps>(
  function ShellLink({ href, onClick, ...props }, ref) {
    const currentShellPath = useAppStore((state) => state.currentShellPath);
    const navigateShellPath = useAppStore((state) => state.navigateShellPath);

    const handleClick = (event: MouseEvent<HTMLAnchorElement>) => {
      onClick?.(event);
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey ||
        props.target === "_blank"
      ) {
        return;
      }

      event.preventDefault();
      if (href !== currentShellPath) {
        navigateShellPath(href);
      }
    };

    return (
      <a
        {...props}
        ref={ref}
        href={buildStaticRouteUrl(href)}
        onClick={handleClick}
      />
    );
  },
);
