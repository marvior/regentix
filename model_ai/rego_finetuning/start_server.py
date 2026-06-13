from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import List, Dict, Any
import uvicorn
from mlx_lm import load, generate
import json
import os
import subprocess

print("🔄 Caricamento del modello Qwen con adapter Rego...")
MODEL_PATH = "Qwen/Qwen2.5-Coder-1.5B-Instruct"
#MODEL_PATH  = "mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit"
ADAPTER_PATH = "adapters"
CONFIG_FILE_PATH = "../../config_metodo.json"
POLICY_DIR = "../../policy"
PROXY_BINARY_PATH = "../../regentix"

# Carichiamo il modello nativamente tramite la libreria funzionante
model, tokenizer = load(MODEL_PATH, adapter_path=ADAPTER_PATH)
print("🚀 Modello e Adapter caricati in memoria con successo!")

app = FastAPI(title="MLX Rego Adapter Server")

# -------------------------------------------------------------
# MODELLI DATI
# -------------------------------------------------------------
class Message(BaseModel):
    role: str
    content: str

class ChatCompletionRequest(BaseModel):
    model: str
    messages: List[Message]
    temperature: float = 0.7

class SavePolicyRequest(BaseModel):
    server_name:str
    method_name: str
    rego_code: str

# -------------------------------------------------------------
# ENDPOINT 1: CHAT STANDARD (Per l'interfaccia Angular)
# -------------------------------------------------------------
@app.post("/v1/chat/completions")
async def chat_completions(request: ChatCompletionRequest):
    try:
        formatted_messages=[{"role": "system", "content": """Sei un assistente esperto in regorust e Open Policy Agent (OPA). Il tuo compito è generare codice Rego valido, nessun commento, sicuro ed efficiente partendo dalle richieste dell'utente."""}]
        formatted_messages_user = [{"role": m.role, "content": m.content} for m in request.messages]
        formatted_messages.extend(formatted_messages_user)
        print(formatted_messages)
        prompt = tokenizer.apply_chat_template(
            formatted_messages, 
            tokenize=False, 
            add_generation_prompt=True
        )
        
        print(f"🧠 Generazione risposta via Chat...")
        risposta_testo = generate(model, tokenizer, prompt=prompt, max_tokens=400, verbose=True)
        
        return {
            "choices": [{"message": {"role": "assistant", "content": risposta_testo}}]
        }
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

# -------------------------------------------------------------
# ENDPOINT 2: GENERA DINAMICAMENTE LA CONFIGURAZIONE SENZA ATTESE DI TIMEOUT
# -------------------------------------------------------------
@app.get("/v1/execute-config")
async def execute_config():
    try:
        payload = {
            "method": "tools/list",
            "params": {},
            "jsonrpc": "2.0",
            "id": 1
        }
        payload_str = json.dumps(payload) + "\n"
        
        print(f"🔌 Avvio binario proxy mcp: {PROXY_BINARY_PATH}")
        if not os.path.exists(PROXY_BINARY_PATH):
            raise HTTPException(
                status_code=500, 
                detail=f"Eseguibile mcp-proxy non trovato al percorso: {PROXY_BINARY_PATH}"
            )
            
        # Avviamo il sottoprocesso
        process = subprocess.Popen(
            [PROXY_BINARY_PATH],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1  # Line buffered
        )
        
        print("📨 Scrittura immediata su stdin...")
        process.stdin.write(payload_str)
        process.stdin.flush()

        json_rpc_response = None
        stdout_lines_recuperate = []

        print("📩 Lettura real-time dello stdout riga per riga...")
        # Leggiamo attivamente l'output senza bloccarci sul fine processo
        while True:
            line = process.stdout.readline()
            if not line:
                break
            
            stdout_lines_recuperate.append(line)
            
            # Se la riga contiene un JSON valido con un risultato, abbiamo fatto!
            if "{" in line and "}" in line:
                try:
                    start_idx = line.find("{")
                    end_idx = line.rfind("}") + 1
                    candidate = line[start_idx:end_idx]
                    parsed = json.loads(candidate)
                    if "result" in parsed or "error" in parsed:
                        json_rpc_response = parsed
                        print("🎯 Risposta JSON-RPC intercettata con successo!")
                        break
                except json.JSONDecodeError:
                    continue

        # Terminiamo immediatamente il processo in background visto che abbiamo il dato
        try:
            process.terminate()
            process.wait(timeout=1)
        except Exception:
            process.kill()

        if not json_rpc_response or "result" not in json_rpc_response:
            output_completo = "".join(stdout_lines_recuperate)
            print(f"❌ Errore: Nessun JSON valido estratto. Output letto: {output_completo}")
            raise HTTPException(status_code=500, detail="Risposta invalida o mancante dal proxy MCP.")

        # Mappatura dei tool
        tools_list = json_rpc_response["result"].get("tools", [])
        mappa_configurazione = []
        
        for tool in tools_list:
            tmp = tool.get("name", "")
            nome_metodo = tmp.split("__")[1]
            server_name= tmp.split("__")[0]
            proprieta_parametri = tool.get("inputSchema", {}).get("properties", {})
            lista_parametri = list(proprieta_parametri.keys())
            description=tool.get("description", "")
            
            mappa_configurazione.append({
                "server_name": server_name,
                "metodo": nome_metodo,
                "parametri": lista_parametri,
                "description":description
            })
            
        print(f"💾 Salvataggio configurazione aggiornata in: {CONFIG_FILE_PATH}")
        with open(CONFIG_FILE_PATH, 'w', encoding='utf-8') as f:
            json.dump(mappa_configurazione, f, indent=4, ensure_ascii=False)

        # Integrazione con i file .rego presenti
        for item in mappa_configurazione:
            method_name = item.get("metodo", "").strip()
            item["aiResult"] = ""  
            
            if method_name:
                safe_name = os.path.basename(method_name)+"_policy"
                filename = safe_name if safe_name.endswith('.rego') else f"{safe_name}.rego"
                filename = item.get("server_name", "").strip()+"__"+filename
                file_path = os.path.join(POLICY_DIR, filename)
                
                if os.path.exists(file_path):
                    try:
                        with open(file_path, "r", encoding="utf-8") as pf:
                            item["aiResult"] = pf.read()
                        print(f"📂 Caricata policy esistente trovata per: {filename}")
                    except Exception as fe:
                        print(f"⚠️ Impossibile leggere file della policy {filename}: {str(fe)}")

        return mappa_configurazione

    except HTTPException as he:
        raise he
    except Exception as e:
        print(f"❌ Errore generale durante l'execute-config: {str(e)}")
        raise HTTPException(status_code=500, detail=str(e))

# -------------------------------------------------------------
# ENDPOINT 3: SALVA LA POLICY REGO NELLA CARTELLA DESTINAZIONE
# -------------------------------------------------------------
@app.post("/v1/save-policy")
async def save_policy(request: SavePolicyRequest):
    try:
        
        safe_name = os.path.basename(request.server_name).strip()+"__"+os.path.basename(request.method_name).strip()+"_policy"
        if not safe_name:
            raise HTTPException(status_code=400, detail="Nome metodo non valido.")
        
        filename = safe_name if safe_name.endswith('_policy.rego') else f"{safe_name}.rego"
        
        os.makedirs(POLICY_DIR, exist_ok=True)
        file_path = os.path.join(POLICY_DIR, filename)
        
        print(f"💾 Scrittura della policy in corso su: {file_path}")
        with open(file_path, "w", encoding="utf-8") as f:
            f.write(request.rego_code)
            
        return {"status": "success", "message": f"File {filename} saved!"}
        
    except Exception as e:
        print(f"❌ Errore durante il salvataggio sul disco: {str(e)}")
        raise HTTPException(status_code=500, detail=str(e))

if __name__ == "__main__":
    uvicorn.run(app, host="127.0.0.1", port=8080)