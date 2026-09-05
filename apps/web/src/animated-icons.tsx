import UseAnimations from "react-useanimations";
import type { Animation } from "react-useanimations/utils";
import activity from "react-useanimations/lib/activity";
import archive from "react-useanimations/lib/archive";
import star from "react-useanimations/lib/star";
import edit from "react-useanimations/lib/edit";
import airplay from "react-useanimations/lib/airplay";
import arrowDown from "react-useanimations/lib/arrowDown";
import arrowRightCircle from "react-useanimations/lib/arrowRightCircle";
import download from "react-useanimations/lib/download";
import help from "react-useanimations/lib/help";
import info from "react-useanimations/lib/info";
import menu3 from "react-useanimations/lib/menu3";
import notification from "react-useanimations/lib/notification";
import playPause from "react-useanimations/lib/playPause";
import plusToX from "react-useanimations/lib/plusToX";
import pocket from "react-useanimations/lib/pocket";
import settings2 from "react-useanimations/lib/settings2";
import skipBack from "react-useanimations/lib/skipBack";
import toggle from "react-useanimations/lib/toggle";
import userPlus from "react-useanimations/lib/userPlus";
import video from "react-useanimations/lib/video";

type IconProps={className?:string;size?:number};

function animatedIcon(animation:Animation,defaultSize=16,reverse=false){
  return function AnimatedIcon({className,size=defaultSize}:IconProps){
    return <UseAnimations
      animation={animation}
      reverse={reverse}
      size={size}
      strokeColor="currentColor"
      className={["animated-icon",className].filter(Boolean).join(" ")}
      aria-hidden="true"
      wrapperStyle={{display:"inline-flex",alignItems:"center",justifyContent:"center",flex:"0 0 auto"}}
    />;
  };
}

export const BotIcon=animatedIcon(userPlus,18);
export const Brain=animatedIcon(activity,17);
export const ChevronDown=animatedIcon(arrowDown,14);
export const ChevronsRight=animatedIcon(arrowRightCircle,17);
export const CircleHelp=animatedIcon(help,16);
export const ClipboardPaste=animatedIcon(download,17);
export const Computer=animatedIcon(airplay,18);
export const Ellipsis=animatedIcon(menu3,18);
export const Info=animatedIcon(info,16);
export const LogOut=animatedIcon(arrowRightCircle,16);
export const Megaphone=animatedIcon(notification,16);
export const Paperclip=animatedIcon(archive,16);
export const Pin=animatedIcon(pocket,14);
export const Sparkle=animatedIcon(star,16);
export const Pencil=animatedIcon(edit,14);
export const Plug=animatedIcon(toggle,16);
export const Plus=animatedIcon(plusToX,17);
export const RefreshCw=animatedIcon(skipBack,14);
export const Settings=animatedIcon(settings2,16);
export const Smartphone=animatedIcon(video,16);
export const Square=animatedIcon(playPause,14);
export const Users=animatedIcon(userPlus,17);
export const X=animatedIcon(plusToX,16,true);

export function Download({className,size=16}:IconProps){
  return <svg className={["animated-icon",className].filter(Boolean).join(" ")} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>;
}
export function Upload({className,size=16}:IconProps){
  return <svg className={["animated-icon",className].filter(Boolean).join(" ")} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>;
}
