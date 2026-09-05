import { lazy, Suspense, type ComponentProps } from "react";
import type Animation from "react-useanimations";
const AnimationPlayer = lazy(() => import("react-useanimations"));
export default function UseAnimations(props: ComponentProps<typeof Animation>) {
  const size=props.size ?? 24;
  return <Suspense fallback={<span aria-hidden="true" style={{display:"inline-block",width:size,height:size,flexShrink:0}}/>}><AnimationPlayer {...props}/></Suspense>;
}
