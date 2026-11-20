import { toast } from "sonner";
import { useApp } from "@/store";
import { ROOT_SPACE_ID } from "@/store/core";

/** Shown wherever something would be created on the main canvas, which holds only projects. */
export const ROOT_ONLY_PROJECTS = "The main canvas holds only projects. Open a project first, or create one with + → Project.";

/** True on the main canvas, where designs, pages and explorations can't be created. */
export const atMainCanvas = (spaceId = useApp.getState().activeSpaceId) => spaceId === ROOT_SPACE_ID;

/** For create actions: false (with a toast saying why) when `spaceId` is the main canvas. */
export function canCreateIn(spaceId = useApp.getState().activeSpaceId): boolean {
  if (!atMainCanvas(spaceId)) return true;
  toast.error("Open a project first", { description: ROOT_ONLY_PROJECTS });
  return false;
}
