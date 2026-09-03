declare module "blobshape" {
  export default function blobshape(options?: { size?:number; growth?:number; edges?:number; seed?:number }): { path:string; seedValue:number };
}
