"use client";
import { useState } from "react";
import { toast } from "sonner";
import { Check, Copy, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { copyText } from "./client-utils";

export function ShareLink({ url, className }: { url: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className={cn("flex items-center gap-1", className)}>
      <Input readOnly value={url} onFocus={(e) => e.target.select()} className="h-7 font-mono text-xs md:text-xs" />
      <Button
        size="sm"
        variant="outline"
        onClick={async () => {
          const ok = await copyText(url);
          if (ok) {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          } else toast.error("Couldn't copy - select the link and copy it manually.");
        }}
      >
        {copied ? <Check /> : <Copy />} {copied ? "Copied" : "Copy"}
      </Button>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Open poll"
        render={<a href={url} target="_blank" rel="noreferrer" />}
        nativeButton={false}
      >
        <ExternalLink />
      </Button>
    </div>
  );
}
