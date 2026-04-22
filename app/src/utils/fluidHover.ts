type HoverableRow = HTMLElement & {
  __rowHoverAnimation?: Animation;
};

const EASE_OUT = "cubic-bezier(0.22, 1, 0.36, 1)";

export function rowHoverEnter(el: HTMLElement) {
  const row = el as HoverableRow;
  row.__rowHoverAnimation?.cancel();
  row.__rowHoverAnimation = row.animate(
    [
      { transform: "translateY(0px)" },
      { transform: "translateY(-1px)" },
    ],
    { duration: 100, easing: EASE_OUT, fill: "forwards" }
  );
}

export function rowHoverLeave(el: HTMLElement) {
  const row = el as HoverableRow;
  row.__rowHoverAnimation?.cancel();
  row.__rowHoverAnimation = row.animate(
    [
      { transform: "translateY(-1px)" },
      { transform: "translateY(0px)" },
    ],
    { duration: 90, easing: EASE_OUT, fill: "forwards" }
  );
}
