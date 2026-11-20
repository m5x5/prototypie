import { Slider as SliderPrimitive } from "@base-ui/react/slider"
import { cn } from "cn"

function Slider({
  className,
  defaultValue,
  value,
  min = 0,
  max = 100,
  showValueWhileDragging = false,
  marks = false,
  ...props
}: SliderPrimitive.Root.Props & {
  /** Stepped look: a dot on the track for every step, filled up to the value. */
  marks?: boolean
  /** Show the value as a small tooltip above the thumb, only while it is being dragged. */
  showValueWhileDragging?: boolean
}) {
  const _values = Array.isArray(value)
    ? value
    : Array.isArray(defaultValue)
      ? defaultValue
      : [min, max]

  const stopCount = Math.round((max - min) / (props.step ?? 1)) + 1

  return (
    <SliderPrimitive.Root
      className={cn("group/slider data-horizontal:w-full data-vertical:h-full", className)}
      data-slot="slider"
      defaultValue={defaultValue}
      value={value}
      min={min}
      max={max}
      thumbAlignment="edge"
      {...props}
    >
      <SliderPrimitive.Control className="relative flex w-full touch-none items-center select-none data-disabled:opacity-50 data-vertical:h-full data-vertical:min-h-40 data-vertical:w-auto data-vertical:flex-col">
        <SliderPrimitive.Track
          data-slot="slider-track"
          className="relative grow overflow-hidden rounded-full bg-muted select-none data-horizontal:h-1 data-horizontal:w-full data-vertical:h-full data-vertical:w-1"
        >
          <SliderPrimitive.Indicator
            data-slot="slider-range"
            className="bg-primary select-none data-horizontal:h-full data-vertical:w-full"
          />
        </SliderPrimitive.Track>
        {marks && (
          // Dots sit where the thumb's centre stops: inset by half the thumb for "edge" alignment.
          <div
            aria-hidden
            className={cn(
              "pointer-events-none absolute top-1/2 h-0",
              props.thumbAlignment === "center" ? "inset-x-0" : "inset-x-1.5",
            )}
          >
            {Array.from({ length: stopCount }, (_, i) => {
              const v = min + i * (props.step ?? 1)
              return (
                <span
                  key={i}
                  style={{ left: `${(i / Math.max(stopCount - 1, 1)) * 100}%` }}
                  className={cn(
                    "absolute size-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full",
                    v <= Math.max(..._values) ? "bg-primary" : "bg-muted-foreground/40",
                  )}
                />
              )
            })}
          </div>
        )}
        {Array.from({ length: _values.length }, (_, index) => (
          <SliderPrimitive.Thumb
            data-slot="slider-thumb"
            key={index}
            className="relative block size-3 shrink-0 rounded-full border border-ring bg-white ring-ring/50 transition-[color,box-shadow] select-none after:absolute after:-inset-2 hover:ring-3 focus-visible:ring-3 focus-visible:outline-hidden active:ring-3 disabled:pointer-events-none disabled:opacity-50"
          >
            {showValueWhileDragging && (
              <span className="pointer-events-none absolute bottom-full left-1/2 mb-2 -translate-x-1/2 rounded-md bg-foreground px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-background opacity-0 transition-opacity group-data-dragging/slider:opacity-100">
                {_values[index]}
              </span>
            )}
          </SliderPrimitive.Thumb>
        ))}
      </SliderPrimitive.Control>
    </SliderPrimitive.Root>
  )
}

export { Slider }
