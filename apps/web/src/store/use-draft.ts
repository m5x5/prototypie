"use client";
import { useApp } from "@/store";

/**
 * A half-typed text kept in the per-browser `drafts` key (persisted locally, survives reloads),
 * instead of component state. Setting "" drops the entry.
 */
export function useDraft(key: string): [string, (text: string) => void] {
  const value = useApp((s) => s.drafts[key] ?? "");
  const setDraft = useApp((s) => s.setDraft);
  const clearDraft = useApp((s) => s.clearDraft);
  return [value, (text: string) => (text ? setDraft(key, text) : clearDraft(key))];
}
