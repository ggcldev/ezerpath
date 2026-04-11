import { animate } from "motion";

const EASE_OUT = [0.22, 1, 0.36, 1] as const;

export function animateViewEnter(el: HTMLElement) {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  animate(
    el,
    { opacity: [0.0, 1], transform: ["translateY(2px)", "translateY(0px)"] },
    { duration: 0.12, easing: EASE_OUT }
  );
}
