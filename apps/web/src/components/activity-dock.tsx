"use client";
import type { ReactNode, RefObject } from "react";
import { Popover } from "@base-ui/react/popover";
import { cn } from "@/lib/utils";

/**
 * One entry of the header's activity dock: a two-line pill (name + live status) that opens a panel anchored to it.
 * Same pill language as the agent capacity pill (rounded-full, border, h-8). Esc, outside click and focus return
 * come from Base UI's Popover. Below xl the status line gives way to `compact` (a short count), so the header fits
 * at 1100 px.
 */
export function DockEntry({
  icon,
  name,
  status,
  compact,
  label,
  open,
  onOpenChange,
  highlight,
  children,
  panelLabel,
  anchor,
}: {
  icon: ReactNode;
  name: string;
  /** Second line, e.g. "44 waiting · 2 generating". */
  status: ReactNode;
  /** Shown instead of the status line below xl. */
  compact?: ReactNode;
  /** Accessible name of the trigger, with the numbers spelled out. */
  label: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Something waits for the user: a filled pill. */
  highlight?: boolean;
  panelLabel: string;
  children: ReactNode;
  /** Open from elsewhere (e.g. a menu item): no pill, the panel anchors to this element and returns focus to it. */
  anchor?: RefObject<HTMLElement | null>;
}) {
  return (
    <Popover.Root open={open} onOpenChange={(o) => onOpenChange?.(o)}>
      {!anchor && (
      <Popover.Trigger
        aria-label={label}
        title={label}
        className={cn(
          "flex h-8 shrink-0 items-center gap-2 rounded-full border px-2.5 text-left text-xs font-medium transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring data-[popup-open]:bg-accent motion-reduce:transition-none",
          highlight && "bg-secondary",
        )}
      >
        <span className="shrink-0 text-muted-foreground [&_svg]:size-3.5" aria-hidden>
          {icon}
        </span>
        <span className="flex min-w-0 flex-col leading-none" aria-hidden>
          <span>{name}</span>
          <span className="mt-0.5 hidden max-w-44 truncate text-[10.5px] font-normal text-muted-foreground xl:block">{status}</span>
        </span>
        {compact && <span className="xl:hidden" aria-hidden>{compact}</span>}
      </Popover.Trigger>
      )}
      <Popover.Portal>
        <Popover.Positioner anchor={anchor} side="bottom" align="end" sideOffset={8} className="z-50 outline-none">
          <Popover.Popup
            aria-label={panelLabel}
            finalFocus={anchor}
            className="flex max-h-[min(78vh,640px)] w-[min(360px,calc(100vw-24px))] origin-(--transform-origin) flex-col overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-xl outline-none duration-100 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 motion-reduce:animate-none"
          >
            {children}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** Panel header: title on the left, a count on the right. */
export function DockHead({ title, count }: { title: string; count?: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-2 border-b px-3.5 py-2.5">
      <Popover.Title className="text-sm font-semibold">{title}</Popover.Title>
      {count !== undefined && <span className="text-xs tabular-nums text-muted-foreground">{count}</span>}
    </div>
  );
}

/** Small number badge for the compact (below xl) trigger. */
export function DockCount({ n }: { n: number }) {
  return (
    <span className="grid h-4 min-w-4 place-items-center rounded-full bg-foreground px-1 text-[10px] leading-none font-semibold tabular-nums text-background">
      {n > 99 ? "99+" : n}
    </span>
  );
}
