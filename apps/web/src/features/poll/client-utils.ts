"use client";
import { create } from "zustand";
import type { CreatePollResponse } from "./schema";

/** Transient UI state for the Decisions panel (not persisted). */
export const usePollUI = create<{
  open: boolean;
  /** The Decisions popover opened from the header's More menu. */
  dockOpen: boolean;
  setDockOpen: (open: boolean) => void;
  tab: "build" | "results";
  resultsPollId: string | null;
  setOpen: (open: boolean) => void;
  setTab: (tab: "build" | "results") => void;
  showResults: (pollId: string) => void;
}>()((set) => ({
  open: false,
  dockOpen: false,
  setDockOpen: (dockOpen) => set({ dockOpen }),
  tab: "build",
  resultsPollId: null,
  setOpen: (open) => set({ open }),
  setTab: (tab) => set({ tab }),
  showResults: (resultsPollId) => set({ open: true, tab: "results", resultsPollId }),
}));

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]", "::1", "0.0.0.0"]);

/** A URL colleagues can open: prefer the LAN address when the designer is browsing via localhost. */
export function shareUrlFor(res: CreatePollResponse): string {
  const here = window.location;
  if (LOCAL_HOSTS.has(here.hostname) && res.lanOrigins.length) return `${res.lanOrigins[0]}${res.path}`;
  return `${here.origin}${res.path}`;
}

/** Clipboard API needs a secure context; fall back to execCommand on plain-http LAN hosts. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  ta.remove();
  return ok;
}
