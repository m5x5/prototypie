"use client";
import { ThemeProvider as NextThemesProvider } from "next-themes";

/** Follows the OS light/dark setting by default (toggles the `.dark` class shadcn's tokens use). */
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  return (
    <NextThemesProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
      {children}
    </NextThemesProvider>
  );
}
