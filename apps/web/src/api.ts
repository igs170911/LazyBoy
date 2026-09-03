export async function api<T>(path:string, options:RequestInit={}):Promise<T>{
  const response=await fetch(path,{...options,headers:{"content-type":"application/json",...options.headers}});
  const text=await response.text(); let body:unknown=null;
  try{body=text?JSON.parse(text):null}catch{body={message:text}}
  if(!response.ok){const message=typeof body==="object"&&body&&"message" in body?String((body as {message:unknown}).message):`${response.status} ${response.statusText}`;throw new Error(message)}
  return body as T;
}
