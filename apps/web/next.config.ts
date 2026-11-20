import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Open the dev server from another device: Next 16 blocks dev assets for any other hostname than localhost.
  // List the hostnames or addresses you use, comma separated, in PROTOTYPIE_DEV_ORIGINS (for example
  // "192.168.1.*,my-mac.local").
  allowedDevOrigins: (process.env.PROTOTYPIE_DEV_ORIGINS ?? "").split(",").map((s) => s.trim()).filter(Boolean),
  // pi-ai loads providers/OAuth helpers with dynamic imports that bundling breaks
  // keep it a plain Node dependency.
  serverExternalPackages: ["@earendil-works/pi-ai"],
  // No free corner for the "N" indicator: logo (top-left), header actions (top-right), zoom controls
  // (bottom-left), inspector (bottom-right). Compile/runtime errors still show.
  devIndicators: false,
  experimental: {
    agentUpgrade: "security",
  },
  cacheComponents: true,
  partialPrefetching: true,
  turbopack: {
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;
