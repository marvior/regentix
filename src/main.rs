use std::io::{self, BufRead, Write, BufReader};
use std::fs::OpenOptions;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::Deserialize;
use std::fs::File;
use std::collections::HashMap;
use serde_json::{json, Value};
use std::process::ChildStdout;
use std::io::StdoutLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use regorus::{Engine, Value as RegoValue}; 
use std::sync::RwLock;
use lazy_static::lazy_static;
use std::fs;
use std::env;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Strutture dati
// ---------------------------------------------------------------------------

//this structure contain the command and args provided by MCP SERVER answer for tools/call by CLAUDE
#[derive(Deserialize, Debug)]
struct McpConfig {
    server_name: String,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
}

//coontains for specific MCP SERVER the stdin channel
struct ServerInputInstance {
    server_name: String,
    child_stdin: std::process::ChildStdin,
}

//contains for specific MCP SERVER the stdout channel
struct ServerOutputInstance {
    server_name: String,
    child_stdout: BufReader<ChildStdout>,
}

#[derive(Clone, Debug)]
struct ServerArguments {
    server_name: String,
    list_arguments: Vec<String>,
}

// ---------------------------------------------------------------------------
// Utilità
// ---------------------------------------------------------------------------

fn ts() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn log(line: &str) {
    let mut path = env::current_exe().expect("Impossibile trovare l'eseguibile");
    path.pop();
    path.push("mcp-proxy.log");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "[{}] {}", ts(), line).unwrap();
}

fn send_to_claude(buffer: &str, parent_stdout: &mut StdoutLock) {
    let raw = buffer.trim();
    if !raw.is_empty() {
        if parent_stdout.write_all(buffer.as_bytes()).is_err() {
            log("[ERROR] Fallito l'inoltro della risposta verso Claude");
        }
        let _ = parent_stdout.flush();
    }
}

// ---------------------------------------------------------------------------
// Avvio processo MCP
// ---------------------------------------------------------------------------

fn load_server_io(server: &McpConfig) -> (ServerInputInstance, ServerOutputInstance) {
    let mut child = Command::new(&server.command)
        .args(&server.args)
        .envs(std::env::vars())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("Impossibile avviare il server '{}': {}", server.server_name, e));

    let child_stdin  = child.stdin.take().expect("Failed to open child stdin");
    let child_stdout = child.stdout.take().expect("Failed to open child stdout");
    let child_stderr = child.stderr.take().expect("Failed to open child stderr");

    let sname = server.server_name.clone();
    thread::spawn(move || {
        let mut reader = BufReader::new(child_stderr);
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let raw = buf.trim();
                    if !raw.is_empty() {
                        log(&format!("[STDERR {}] {}", sname, raw));
                    }
                }
            }
        }
    });    

    (
        ServerInputInstance { server_name: server.server_name.clone(), child_stdin },
        ServerOutputInstance { server_name: server.server_name.clone(), child_stdout: BufReader::new(child_stdout) },
    )
}

lazy_static! {
    static ref POLICIES_IN_RAM: RwLock<HashMap<String, String>> = RwLock::new(
        carica_tutte_le_policy("/Users/waltermolino/rustProjects/mcp-proxy/policy")
    );
}


//load and return a dictionary the policies .rego in floder policy
//the key dictionary is name file and the value is the script 
//this structure is useful to check the rego.
fn carica_tutte_le_policy(percorso: &str) -> HashMap<String, String> {
    let mut mappa_policies = HashMap::new();

    if let Ok(voci) = fs::read_dir(percorso) {
        for voce in voci {
            if let Ok(file) = voce {
                let path = file.path();

                if path.is_file() && path.extension().map_or(false, |ext| ext == "rego") {
                    if let Some(nome_file) = path.file_name().and_then(|n| n.to_str()) {
                        if let Ok(contenuto) = fs::read_to_string(&path) {
                            mappa_policies.insert(nome_file.to_string(), contenuto);
                            log(&format!("[RAM] Caricato file policy: {}", nome_file));
                        }
                    }
                }
            }
        }
    }

    if mappa_policies.is_empty() {
        println!("[ATTENZIONE] Nessun file .rego trovato! Carico policy di default.");
        mappa_policies.insert(
            "default.rego".to_string(), 
            "package filesystem.rules\ndefault allow := false".to_string()
        );
    }

    mappa_policies
}



/// this function check the policy for specific rego file
// return true or false from the excution the rego policy
fn controlla_con_regorus_dinamico(
    mappe_lock: &HashMap<String, String>,
    nome_file_rego: &str,
    azione: &str,
    path: &str,
    extra_param: Option<(&str, &str)>,
) -> bool {
    log("SONO QUI 2");

    let policy_testo = match mappe_lock.get(nome_file_rego) {
        Some(contenuto) => contenuto,
        None => {
            log(&format!("[ERRORE] Il file {} non esiste in RAM!", nome_file_rego));
            return true;
        }
    };

    log("SONO QUI 3");

    let mut engine = Engine::new();

    if let Err(e) = engine.add_policy("policy.rego".to_string(), policy_testo.clone()) {
        log(&format!("[ERRORE POLICY] {}", e));
        return false;
    }

    log("SONO QUI 4");

    // path deve contenere un JSON object tipo:
    // {"query":"machine learning language:python","page":2}
    let mut argomenti_mappa: serde_json::Value =
        serde_json::from_str(path).unwrap_or_else(|_| json!({}));

    // Assicuriamoci che sia un object
    if !argomenti_mappa.is_object() {
        argomenti_mappa = json!({});
    }

    if let Some((chiave, valore)) = extra_param {
        if let Some(obj) = argomenti_mappa.as_object_mut() {
            obj.insert(chiave.to_string(), json!(valore));
        }
    }

    log("SONO QUI 5");

    let seconda_azione = azione
        .split_once("__")
        .map(|(_, seconda)| seconda)
        .unwrap_or(azione);

    log(&format!(
        "[seconda azione {}] {:#?}",
        seconda_azione,
        &argomenti_mappa
    ));

    let input_json = json!({
        "action": seconda_azione,
        "args": argomenti_mappa
    });

    log(&format!("INPUT JSON: {}", input_json));

    log("SONO QUI 6");

    let input_value = match RegoValue::from_json_str(&input_json.to_string()) {
        Ok(v) => v,
        Err(e) => {
            log(&format!("[ERRORE INPUT REGORUS] {}", e));
            return false;
        }
    };

    log(&format!("input_value {}", input_value));

    engine.set_input(input_value);

    match engine.eval_rule("data.regorus.policy.decision".to_string()) {
        Ok(value) => {
            log(&format!("Decision value: {:?}", value));
            value.as_bool().copied().unwrap_or(false)
        }
        Err(e) => {
            log(&format!("[ERRORE REGORUS] Fallimento valutazione: {}", e));
            false
        }
    }
}

// ---------------------------------------------------------------------------
// MAIN
// ---------------------------------------------------------------------------

fn main() {
    log("===== MCP PROXY STARTED =====");

    //LOAD the config.json file in the list mcp_configs with the mcp_config_structure
    let mut path = env::current_exe().expect("Impossibile trovare l'eseguibile");
    path.pop();
    path.push("config.json");
    log(&format!("PATH {:?}",path));

    let file = File::open(path)
        .expect("Errore apertura config.json");
    let reader = BufReader::new(file);
    let mcp_configs: Vec<McpConfig> = serde_json::from_reader(reader)
        .expect("Errore nel parsing del JSON");

    //count the total MCP SERVER
    let total_mcp = mcp_configs.len();
    log(&format!("[INFO] Server MCP configurati: {}", total_mcp));

    //list_methotds is shared dictioary with key the method name and value ServerArguments(server_name, list_arguments)
    //the method name is a composed by serverName__methodName
    //this is a concurrency pattern 
    let list_methods: Arc<Mutex<HashMap<String, ServerArguments>>> =
        Arc::new(Mutex::new(HashMap::new()));

    //atomic shared flag
    let running = Arc::new(AtomicBool::new(true));

    //channel transmit and receive information from specific MCP SERVER to the PROXY
    let (tx, rx) = mpsc::channel::<(String, String)>();

    //save the stdin and stdout channel from MCP Server in 2 dictionaries
    let mut stdin_mcp_server:  HashMap<String, ServerInputInstance>  = HashMap::new();
    let mut stdout_mcp_server: HashMap<String, ServerOutputInstance> = HashMap::new();

    for cfg in &mcp_configs {
        let (inp, out) = load_server_io(cfg);
        stdin_mcp_server.insert(cfg.server_name.clone(), inp);
        stdout_mcp_server.insert(cfg.server_name.clone(), out);
    }



    //create a thread for each MCP SERVER
    //each thread will have the copy of TX channel
    //each thread is a listner for the output for specific MCP SERVER
    //the thread send on TX channel the aswer from MCP SERVER
    //PROXY SERVER receive this information from RX
    for (server_name, instance) in stdout_mcp_server {
        let tx_clone      = tx.clone();
        let running_clone = Arc::clone(&running);

        thread::spawn(move || {
            let mut reader = instance.child_stdout;
            let mut buffer = String::new();

            loop {
                buffer.clear();
                match reader.read_line(&mut buffer) {
                    Ok(0) => {
                        log(&format!("[SERVER {}] EOF ricevuto", server_name));
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                    Ok(_) => {
                        let raw = buffer.trim().to_string();
                        log(&format!("HOLA {} {}", server_name, raw));
                        
                        if !raw.is_empty() {
                            log(&format!("[SERVER {} -> PROXY] {}", server_name, raw));
                            
                            //send the answer MCP SERVER to PROXY by TX channel
                            if tx_clone.send((server_name.clone(), raw)).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        log(&format!("[ERROR SERVER {}] Lettura fallita: {}", server_name, e));
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });
    }

// ---------------------------------------------------------------------------
// CLAUDE -> PROXY 
// proxy listen the request from claude.
// there are 2 requests:
// 1) initialize: the proxy send in broadcast to all mcp server
// 2) tools/call: the proxy send the request to specific mcp server  
// ---------------------------------------------------------------------------

    //shared structure list_methods
    let list_methods_input = Arc::clone(&list_methods);

    let stdin_mcp_server_shared: Arc<Mutex<HashMap<String, ServerInputInstance>>> =
        Arc::new(Mutex::new(stdin_mcp_server));
    let running_input     = Arc::clone(&running);
    
    thread::spawn(move || {
        let stdin  = io::stdin();
        let mut reader = stdin.lock();
        let mut buffer = String::new();

        loop {
            buffer.clear();
            match reader.read_line(&mut buffer) {
                Ok(0) => {
                    log("[CLAUDE] EOF — connessione chiusa");
                    running_input.store(false, Ordering::SeqCst);
                    break;
                }
                Ok(_) => {
                    let mut raw = buffer.trim().to_string();
                    if raw.is_empty() {
                        continue;
                    }


                    //transform input claude in json
                    let v: Value = match serde_json::from_str(&raw) {
                        Ok(val) => val,
                        Err(e)  => {
                            log(&format!("[ERROR INPUT] JSON non valido: {} | raw: {}", e, raw));
                            continue;
                        }
                    };

                    log(&format!("[CLAUDE -> PROXY] {}", raw));

                    // Acquisiamo il lock di lettura delle policy all'inizio dell'elaborazione del messaggio
                    let mappe_lock = POLICIES_IN_RAM.read().unwrap();

                    //-----------------
                    //CLAUDE (tool request) -> PROXY
                    //-----------------
                    //if CLAUDE send request tools in target_server the name server for the tool saved in list_methods_input
                    let target_server: Option<String> = if v["method"] == "tools/call" {
                        let tool_server_name = v["params"]["name"].as_str().unwrap_or("").to_string();
                        let mut tool_name = v["params"]["name"].as_str().unwrap_or("").to_string();
                        
                        if let Some((_prima, seconda)) = &tool_name.split_once("__") {
                            raw = raw.replace(&format!("{}__", _prima), "");
                            tool_name = seconda.to_string();
                            
                            
                        }
                        let map = list_methods_input.lock().unwrap();
                        let founds : Option<ServerArguments> = map.get(&tool_server_name).cloned();
                        let found = founds.as_ref().map(|f| f.server_name.clone());
                        log(&format!("[ROUTING] tool='{}' -> server={:?}", tool_name, found));
                        found
                    } else {
                        None
                    };

                    let mut inputs = stdin_mcp_server_shared.lock().unwrap();

                    match target_server {
                        //if target_server contain a name then request is 
                        Some(ref sname) => { 
                            //2) tools/call: the proxy send the request to specific mcp server
                            
                            if let Some(instance) = inputs.get_mut(sname) {
                                let line = format!("{}\n", raw);
                                let action = v["params"]["name"].as_str().unwrap_or("").to_string();
                                let param = v["params"]["arguments"].to_string();
                                log(&format!("SONO QUI CIAO {} {}",  action.clone(), param.clone()));
                                let rego_filename = action.clone() + "_policy.rego";
                                
                                // Passiamo mappe_lock come primo argomento
                                let ok_1 = controlla_con_regorus_dinamico(&mappe_lock, &rego_filename, &action, &param, None);
                                
                                log(&format!("{} in {} consentita? -> {}", action, param, ok_1));
                                if !ok_1 {
                                    let text1 = r#"{"result":{"content":[{"type":"text","text":""#;
                                    let text2 = r#""}],"structuredContent":{"content":""#;
                                    let text3 = r#""}},"jsonrpc":"2.0","id":"#;
                                    let text4_id = "}";
                                    let id_number = v["id"].as_i64().unwrap_or(0);
                                    
                                    let text = format!("{}ERROR PERMISSION DENIED{}ERROR PERMISSION DENIED{}{}{}", text1, text2, text3, id_number, text4_id);
                                    log(&format!("ERROR SEND for method {} {}",action, text));

                                    //SIMULATE THE SERVER ANSWER USING THE CHANNEL TX
                                    if tx.send((sname.clone(), text)).is_err() {
                                        break;
                                    }
                                    continue;
                                } else {
                                    log("TRUE");
                                }
                                if instance.child_stdin.write_all(line.as_bytes()).is_err() {
                                    log(&format!("[ERROR] Scrittura su {} fallita", sname));
                                } else {
                                    let _ = instance.child_stdin.flush();
                                    log(&format!("[OK] Inoltrato a {}", sname));
                                }
                            } else {
                                log(&format!("[WARN] Server '{}' non trovato in stdin_mcp_server", sname));
                            }
                        }
                        None => {
                            //1) initialize: the proxy send in broadcast to all mcp server
                            for (sname, instance) in inputs.iter_mut() {
                                let line = format!("{}\n", raw);
                                if instance.child_stdin.write_all(line.as_bytes()).is_err() {
                                    log(&format!("[ERROR] Scrittura su {} fallita", sname));
                                } else {
                                    let _ = instance.child_stdin.flush();
                                    log(&format!("[OK] Broadcast a {}", sname));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log(&format!("[ERROR INPUT] Lettura da Claude fallita: {}", e));
                    running_input.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
    });

    // -----------------------------------------------------------------------
    // MAIN THREAD: riceve dal canale mpsc e aggrega / inoltra a Claude
    // -----------------------------------------------------------------------
    let stdout = io::stdout();
    let mut parent_stdout = stdout.lock();

    let mut claude_tools_response: Value = json!({
        "result": { "tools": [] },
        "jsonrpc": "2.0",
        "id": 1
    });
    let mut tools_received = 0usize;
    let mut init_received  = 0usize;
    
    //infinite cycle about rx channel
    for (server_name, raw) in &rx {
        let v: Value = match serde_json::from_str(&raw) {
            Ok(val) => val,
            Err(e)  => {
                log(&format!("[ERROR MAIN] JSON non valido da {}: {} | {}", server_name, e, raw));
                continue;
            }
        };


        //send  just 1 message for all MCP SERVER
        if !v["result"]["serverInfo"]["name"].is_null() {
            init_received += 1;
            log(&format!("[INIT] Server {} ({}/{})", server_name, init_received, total_mcp));

            if init_received == total_mcp {
                let mut unified = v.clone();
                unified["result"]["serverInfo"]["name"] = json!("proxy_wally");
                let out = format!("{}\n", serde_json::to_string(&unified).unwrap());
                log(&format!("[PROXY -> CLAUDE] initialize: {}", out.trim()));
                send_to_claude(&out, &mut parent_stdout);
                init_received = 0;
            }
            continue;
        }

        //send one shot messagge with all answer MCP SERVER regarding tool list
        if !v["result"]["tools"].is_null() {
            let mut tools = v["result"]["tools"].clone();
            tools_received += 1;
            log(&format!("[TOOLS] Ricevuti tools da {} ({}/{})", server_name, tools_received, total_mcp));

            if let Some(arr) = tools.as_array_mut() {

                
                for ar in &mut *arr {
                    // 3. Accedi alla chiave "name" in modo mutabile se esiste ed è una stringa
                    if let Some(name) = ar["name"].as_str() {
                        // 4. Crea la nuova stringa concatenata
                        let new_name = format!("{}__{}", server_name,name);
                        // 5. Sostituisci il vecchio valore con il nuovo JSON String
                        ar["name"] = serde_json::Value::String(new_name);
                    }
                }
                




                claude_tools_response["result"]["tools"]
                    .as_array_mut()
                    .unwrap()
                    .extend(arr.iter().cloned());

                let mut map = list_methods.lock().unwrap();
                for tool in arr {
                    if let Some(name) = tool["name"].as_str() {
                        if let Some(properties) = tool["inputSchema"]["required"].as_array() {
                            let required_fields: Vec<String> = properties.iter()
                                .filter_map(|v| v.as_str())
                                .map(|s| s.to_string())
                                .collect();
                        
                            let server_argument = ServerArguments {
                                server_name: server_name.clone(),
                                list_arguments: required_fields,
                            };
                            map.insert(name.to_string(), server_argument.clone());
                            log(&format!("[MAP] '{}' -> '{}' {:?}", name, server_name, server_argument.clone()));
                        }
                    }
                }
            }

            if tools_received == total_mcp {
                claude_tools_response["id"] = v["id"].clone();
                let out = format!("{}\n", serde_json::to_string(&claude_tools_response).unwrap());
                log(&format!("[PROXY -> CLAUDE] tools/list: {}", out.trim()));
                send_to_claude(&out, &mut parent_stdout);

                tools_received = 0;
                claude_tools_response = json!({
                    "result": { "tools": [] },
                    "jsonrpc": "2.0",
                    "id": 1
                });
            }
            continue;
        }

        //send tool response
        if !v["result"]["content"].is_null() {
            let out = format!("{}\n", serde_json::to_string(&v).unwrap());
            log(&format!("[PROXY -> CLAUDE] tools/call result da {}: {}", server_name, out.trim()));
            send_to_claude(&out, &mut parent_stdout);
            continue;
        }

        if !v["error"].is_null() {
            let out = format!("{}\n", serde_json::to_string(&v).unwrap());
            log(&format!("[PROXY -> CLAUDE] error da {}: {}", server_name, out.trim()));
            send_to_claude(&out, &mut parent_stdout);
            continue;
        }

        log(&format!("[PROXY] Messaggio non classificato da {}: {}", server_name, raw));
        let out = format!("{}\n", raw);
        send_to_claude(&out, &mut parent_stdout);
    }

    log("===== MCP PROXY STOPPED =====");
}