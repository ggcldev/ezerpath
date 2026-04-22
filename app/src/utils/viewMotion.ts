const EASE_OUT = "cubic-bezier(0.22, 1, 0.36, 1)";

export function animateViewEnter(el: HTMLElement) {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  el.animate(
    [
      { opacity: 0, transform: "translateY(2px)" },
      { opacity: 1, transform: "translateY(0px)" },
    ],
    { duration: 120, easing: EASE_OUT, fill: "both" }
  );
}
