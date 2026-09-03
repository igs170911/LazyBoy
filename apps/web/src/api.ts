export class ApiError extends Error {
  status:number;
  constructor(status:number,message:string){super(message);this.name="ApiError";this.status=status}
}

export async function api<T>(path:string, options:RequestInit={}):Promise<T>{
  const response=await fetch(path,{...options,headers:{"content-type":"application/json",...options.headers}});
  const text=await response.text(); let body:unknown=null;
  try{body=text?JSON.parse(text):null}catch{body={message:text}}
  if(!response.ok){const message=typeof body==="object"&&body&&"message" in body?String((body as {message:unknown}).message):`${response.status} ${response.statusText}`;throw new ApiError(response.status,message)}
  return body as T;
}
