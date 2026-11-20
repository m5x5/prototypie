"use client";
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Mic, MicOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { isTypingTarget } from "./lib";

// Minimal Web Speech API typings (not in every TS DOM lib, and vendor-prefixed in Chromium/Safari).
interface RecognitionAlternative {
  transcript: string;
}
interface RecognitionResult {
  readonly isFinal: boolean;
  readonly length: number;
  [index: number]: RecognitionAlternative;
}
interface RecognitionEvent {
  readonly resultIndex: number;
  readonly results: { readonly length: number; [index: number]: RecognitionResult };
}
interface Recognition {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((e: RecognitionEvent) => void) | null;
  onerror: ((e: { error: string }) => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
}
type RecognitionCtor = new () => Recognition;

function getRecognitionCtor(): RecognitionCtor | undefined {
  if (typeof window === "undefined") return undefined;
  const w = window as unknown as { SpeechRecognition?: RecognitionCtor; webkitSpeechRecognition?: RecognitionCtor };
  return w.SpeechRecognition ?? w.webkitSpeechRecognition;
}

const noopSubscribe = () => () => {};
/** True when the browser supports speech recognition (false during SSR). */
export function useVoiceSupported() {
  return useSyncExternalStore(
    noopSubscribe,
    () => !!getRecognitionCtor(),
    () => false,
  );
}

/**
 * Dictation button (Web Speech API, continuous with interim results).
 * `onText` receives the cumulative transcript of the current dictation session (final + interim), so a
 * consumer can do `setValue(join(valueAtStart, text))`. `onStart` fires when listening begins.
 */
export function VoiceButton({
  onText,
  onStart,
  onEnd,
  hotkey,
  className,
  size = "icon-sm",
}: {
  onText: (text: string) => void;
  onStart?: () => void;
  onEnd?: () => void;
  /** Optional single-key shortcut (ignored while typing in inputs). */
  hotkey?: string;
  className?: string;
  size?: "icon-sm" | "icon" | "icon-xs";
}) {
  const supported = useVoiceSupported();
  const [listening, setListening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recRef = useRef<Recognition | null>(null);
  const cb = useRef({ onText, onStart, onEnd });
  useEffect(() => {
    cb.current = { onText, onStart, onEnd };
  });

  const stop = useCallback(() => recRef.current?.stop(), []);

  const start = useCallback(() => {
    const Ctor = getRecognitionCtor();
    if (!Ctor) return;
    const rec = new Ctor();
    rec.continuous = true;
    rec.interimResults = true;
    rec.lang = navigator.language || "en-US";
    let finalText = "";
    rec.onresult = (e) => {
      let interim = "";
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const r = e.results[i];
        if (r.isFinal) finalText = joinWords(finalText, r[0].transcript);
        else interim = joinWords(interim, r[0].transcript);
      }
      cb.current.onText(joinWords(finalText, interim));
    };
    rec.onerror = (e) => {
      if (e.error === "aborted" || e.error === "no-speech") return;
      setError(
        e.error === "not-allowed" || e.error === "service-not-allowed"
          ? "Microphone access was blocked"
          : `Voice input failed (${e.error})`,
      );
    };
    rec.onend = () => {
      recRef.current = null;
      setListening(false);
      cb.current.onText(finalText);
      cb.current.onEnd?.();
    };
    try {
      rec.start();
      recRef.current = rec;
      setError(null);
      setListening(true);
      cb.current.onStart?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not start voice input");
    }
  }, []);

  const toggle = useCallback(() => (recRef.current ? stop() : start()), [start, stop]);

  useEffect(() => () => recRef.current?.abort(), []);

  useEffect(() => {
    if (!hotkey || !supported) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || isTypingTarget(e.target)) return;
      if (e.key.toLowerCase() === hotkey.toLowerCase()) {
        e.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [hotkey, supported, toggle]);

  const label = !supported
    ? "Voice input isn't supported in this browser"
    : error
      ? error
      : listening
        ? "Stop dictation"
        : `Dictate${hotkey ? ` (${hotkey.toUpperCase()})` : ""}`;

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            type="button"
            size={size}
            variant={listening ? "default" : "ghost"}
            aria-label={label}
            aria-pressed={listening}
            disabled={!supported}
            onClick={toggle}
            className={cn(
              listening && "animate-pulse bg-rose-500 text-white hover:bg-rose-500/90",
              error && !listening && "text-destructive",
              className,
            )}
          />
        }
      >
        {supported ? <Mic /> : <MicOff />}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function joinWords(a: string, b: string) {
  const x = a.trim();
  const y = b.trim();
  return x && y ? `${x} ${y}` : x || y;
}

/** Appends dictated text to existing text with a single space. */
export function appendDictation(base: string, dictated: string) {
  return joinWords(base, dictated);
}
