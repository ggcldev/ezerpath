import { animate, type AnimationPlaybackControls } from "motion";

type HoverableRow = HTMLElement & {
  __rowHoverAnimation?: AnimationPlaybackControls;
};

const EASE_OUT = [0.22, 1, 0.36, 1] as const;

export function rowHoverEnter(el: HTMLElement) {
  const row = el as HoverableRow;
  row.__rowHoverAnimation?.stop();
  row.__rowHoverAnimation = animate(
    row,
    { transform: "translateY(-0.5px)" },
    { duration: 0.1, easing: EASE_OUT }
  );
}

export function rowHoverLeave(el: HTMLElement) {
  const row = el as HoverableRow;
  row.__rowHoverAnimation?.stop();
  row.__rowHoverAnimation = animate(
    row,
    { transform: "translateY(0px)" },
    { duration: 0.09, easing: EASE_OUT }
  );
}
