import type { Metadata, Viewport } from "next";
import { Geist_Mono, Google_Sans_Flex } from "next/font/google";
import "@xyflow/react/dist/style.css";
import "./globals.css";
import { ThemeProvider } from "@/components/theme-provider";

const googleSansFlex = Google_Sans_Flex({
  variable: "--font-google-sans-flex",
  subsets: ["latin"],
  axes: ["opsz", "wdth", "GRAD", "ROND"],
  adjustFontFallback: false,
  fallback: ["system-ui", "sans-serif"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Prototypie",
  description: "Explore AI-proposed design variations on a canvas",
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  viewportFit: "cover",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${googleSansFlex.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col bg-background text-foreground">
        <ThemeProvider>{children}</ThemeProvider>
      </body>
    </html>
  );
}
