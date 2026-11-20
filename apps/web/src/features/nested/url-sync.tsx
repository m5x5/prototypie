"use client";
import { useEffect } from "react";
import { useApp } from "@/store";
import { enterSpace } from "./nav";

/**
 * Mirrors the active space in the URL (?space=<id>&s=<name-slug>), so a reload or a shared
 * link lands in the same nested canvas, and browser back/forward walks the space history.
 */
export function SpaceUrlSync() {
  useEffect(() => {
    const slug = (name: string) => name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40);
    const urlFor = (spaceId: string) => {
      const url = new URL(window.location.href);
      const space = useApp.getState().spaces[spaceId];
      if (!space || !space.parentSpaceId) {
        url.searchParams.delete("space");
        url.searchParams.delete("s");
      } else {
        url.searchParams.set("space", spaceId);
        url.searchParams.set("s", slug(space.name));
      }
      return url.toString();
    };
    const fromUrl = () => new URL(window.location.href).searchParams.get("space");

    let unsub: (() => void) | undefined;
    const start = () => {
      // 1. Land on the space in the URL (if it exists in this browser's data).
      const wanted = fromUrl();
      if (wanted && useApp.getState().spaces[wanted]) enterSpace(wanted);
      else window.history.replaceState(null, "", urlFor(useApp.getState().activeSpaceId));
      // 2. Push a history entry whenever the active space (or its name) changes.
      unsub = useApp.subscribe((s, prev) => {
        const nameChanged = s.spaces[s.activeSpaceId]?.name !== prev.spaces[s.activeSpaceId]?.name;
        if (s.activeSpaceId === prev.activeSpaceId && !nameChanged) return;
        const next = urlFor(s.activeSpaceId);
        if (next === window.location.href) return;
        if (s.activeSpaceId !== prev.activeSpaceId) window.history.pushState(null, "", next);
        else window.history.replaceState(null, "", next);
      });
    };
    const onPop = () => {
      const id = fromUrl();
      enterSpace(id && useApp.getState().spaces[id] ? id : "root");
    };
    window.addEventListener("popstate", onPop);
    if (useApp.persist.hasHydrated()) start();
    const unhydrate = useApp.persist.onFinishHydration(() => {
      if (!unsub) start();
    });
    return () => {
      window.removeEventListener("popstate", onPop);
      unhydrate();
      unsub?.();
    };
  }, []);
  return null;
}
