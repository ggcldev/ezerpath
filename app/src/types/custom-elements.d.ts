import "solid-js";

declare module "solid-js" {
  namespace JSX {
    interface IntrinsicElements {
      "number-flow": any;
      "number-flow-group": any;
    }
  }
}
