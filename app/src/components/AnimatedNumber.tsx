import { createEffect } from "solid-js";
import "number-flow";

type NumberFlowElement = HTMLElement & {
  update: (value?: number | string) => void;
  format?: Intl.NumberFormatOptions;
  locales?: Intl.LocalesArgument;
  animated?: boolean;
  trend?: number;
};

interface AnimatedNumberProps {
  value: number;
  class?: string;
  format?: Intl.NumberFormatOptions;
  locales?: Intl.LocalesArgument;
  animated?: boolean;
  trend?: number;
}

const SNAPPY_TRANSFORM: EffectTiming = {
  duration: 288,
  easing: "cubic-bezier(0.22, 1, 0.36, 1)",
};

const SNAPPY_SPIN: EffectTiming = {
  duration: 336,
  easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
};

const SNAPPY_OPACITY: EffectTiming = {
  duration: 144,
  easing: "ease-out",
};

export default function AnimatedNumber(props: AnimatedNumberProps) {
  let flowEl: NumberFlowElement | undefined;

  createEffect(() => {
    if (!flowEl) return;
    flowEl.locales = props.locales;
    flowEl.format = props.format;
    flowEl.animated = props.animated ?? true;
    flowEl.trend = props.trend;
    (flowEl as unknown as { transformTiming?: EffectTiming }).transformTiming = SNAPPY_TRANSFORM;
    (flowEl as unknown as { spinTiming?: EffectTiming }).spinTiming = SNAPPY_SPIN;
    (flowEl as unknown as { opacityTiming?: EffectTiming }).opacityTiming = SNAPPY_OPACITY;
    flowEl.update(props.value);
  });

  return <number-flow ref={(el: NumberFlowElement) => { flowEl = el; }} class={props.class} />;
}
