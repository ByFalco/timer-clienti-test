#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use slint::{ModelRc, VecModel, Model};
use std::rc::Rc;
use std::time::Instant;
use uuid::Uuid;

slint::include_modules!();

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ClientDbData {
    id: String,
    name: String,
    last_duration_secs: u64,
}

fn format_duration(secs: u64) -> String {
    if secs == 0 {
        return String::from("-");
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = sled::open("timer-clienti.db")?;

    let app = App::new().unwrap();
    let clients_model = Rc::new(VecModel::default());

    // Load from DB
    let mut db_clients = Vec::new();
    for res in db.iter() {
        if let Ok((k, v)) = res {
            let key_str = String::from_utf8_lossy(&k);
            if key_str.starts_with("client:") {
                if let Ok(client) = serde_json::from_slice::<ClientDbData>(&v) {
                    let client_data = ClientData {
                        id: client.id.into(),
                        name: client.name.into(),
                        last_duration_str: format_duration(client.last_duration_secs).into(),
                        is_running: false,
                    };
                    db_clients.push(client_data);
                }
            }
        }
    }
    db_clients.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    clients_model.set_vec(db_clients);

    app.set_clients(ModelRc::from(clients_model.clone()));

    // (client_id, start_time)
    let active_timer = Rc::new(std::cell::RefCell::new(None::<(String, Instant)>));

    let app_weak = app.as_weak();
    let db_clone = db.clone();
    let clients_model_clone = clients_model.clone();
    let active_timer_clone = active_timer.clone();

    app.on_toggle_timer(move |id, is_running| {
        let _app = app_weak.unwrap();
        let mut timer = active_timer_clone.borrow_mut();
        let id_str = id.to_string();

        if !is_running {
            if let Some((active_id, start_time)) = timer.take() {
                let duration = start_time.elapsed().as_secs();
                if let Ok(Some(db_data)) = db_clone.get(format!("client:{}", active_id)) {
                    if let Ok(mut c_data) = serde_json::from_slice::<ClientDbData>(&db_data) {
                        c_data.last_duration_secs = duration;
                        let _ = db_clone.insert(format!("client:{}", active_id), serde_json::to_vec(&c_data).unwrap());
                        let _ = db_clone.flush();
                        
                        _app.set_last_client_duration(format_duration(duration).into());
                        
                        for i in 0..clients_model_clone.row_count() {
                            let mut row = clients_model_clone.row_data(i).unwrap();
                            // match with string
                            if row.id == active_id.as_str() {
                                row.is_running = false;
                                row.last_duration_str = format_duration(duration).into();
                                clients_model_clone.set_row_data(i, row);
                                break;
                            }
                        }
                    }
                }
            }

            *timer = Some((id_str.clone(), Instant::now()));
            for i in 0..clients_model_clone.row_count() {
                let mut row = clients_model_clone.row_data(i).unwrap();
                if row.id == id_str.as_str() {
                    row.is_running = true;
                    clients_model_clone.set_row_data(i, row);
                    break;
                }
            }

        } else {
            if let Some((active_id, start_time)) = timer.take() {
                if active_id == id_str {
                    let duration = start_time.elapsed().as_secs();
                    if let Ok(Some(db_data)) = db_clone.get(format!("client:{}", active_id)) {
                        if let Ok(mut c_data) = serde_json::from_slice::<ClientDbData>(&db_data) {
                            c_data.last_duration_secs = duration;
                            let _ = db_clone.insert(format!("client:{}", active_id), serde_json::to_vec(&c_data).unwrap());
                            let _ = db_clone.flush();

                            _app.set_last_client_duration(format_duration(duration).into());

                            for i in 0..clients_model_clone.row_count() {
                                let mut row = clients_model_clone.row_data(i).unwrap();
                                if row.id == active_id.as_str() {
                                    row.is_running = false;
                                    row.last_duration_str = format_duration(duration).into();
                                    clients_model_clone.set_row_data(i, row);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let db_clone_2 = db.clone();
    let clients_model_clone_2 = clients_model.clone();
    
    app.on_add_client(move |name| {
        let name_str = name.to_string();
        let uuid = Uuid::new_v4().to_string();
        
        let new_client = ClientDbData {
            id: uuid.clone(),
            name: name_str.clone(),
            last_duration_secs: 0,
        };
        
        let _ = db_clone_2.insert(format!("client:{}", uuid), serde_json::to_vec(&new_client).unwrap());
        let _ = db_clone_2.flush();

        let client_data = ClientData {
            id: uuid.as_str().into(),
            name: name.clone(),
            last_duration_str: format_duration(0).into(),
            is_running: false,
        };
        
        let mut current_clients: Vec<_> = clients_model_clone_2.iter().collect();
        current_clients.push(client_data);
        current_clients.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        clients_model_clone_2.set_vec(current_clients);
    });

    let db_clone_3 = db.clone();
    let clients_model_clone_3 = clients_model.clone();
    let active_timer_clone_3 = active_timer.clone();

    app.on_delete_client(move |id| {
        let id_str = id.to_string();
        
        // Verifica se il timer è attivo per questo cliente (sicurezza extra backend)
        let is_running = {
            let timer = active_timer_clone_3.borrow();
            timer.as_ref().map(|(active_id, _)| active_id == &id_str).unwrap_or(false)
        };
        
        if is_running {
            return;
        }

        let _ = db_clone_3.remove(format!("client:{}", id_str));
        let _ = db_clone_3.flush();

        let mut row_to_remove = None;
        for i in 0..clients_model_clone_3.row_count() {
            let row = clients_model_clone_3.row_data(i).unwrap();
            if row.id == id_str.as_str() {
                row_to_remove = Some(i);
                break;
            }
        }
        
        if let Some(index) = row_to_remove {
            clients_model_clone_3.remove(index);
        }
    });

    let db_import = db.clone();
    let clients_model_import = clients_model.clone();
    app.on_import_csv(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file() {
            if let Ok(mut rdr) = csv::ReaderBuilder::new().has_headers(false).from_path(path) {
                for (i, result) in rdr.records().enumerate() {
                    if let Ok(record) = result {
                        let name = record.get(0).unwrap_or("").trim().to_string();
                        
                        // Salta eventuale intestazione (es: "nomi", "clienti")
                        if i == 0 && (name.to_lowercase() == "nomi" || name.to_lowercase() == "clienti") {
                            continue;
                        }
                        
                        if name.is_empty() { continue; }
                        
                        let id = Uuid::new_v4().to_string();
                        let duration = 0;
                        
                        let new_client = ClientDbData {
                            id: id.clone(),
                            name: name.clone(),
                            last_duration_secs: duration,
                        };
                        
                        let _ = db_import.insert(format!("client:{}", id), serde_json::to_vec(&new_client).unwrap());
                        
                        let client_data = ClientData {
                            id: id.as_str().into(),
                            name: name.into(),
                            last_duration_str: format_duration(duration).into(),
                            is_running: false,
                        };
                        clients_model_import.push(client_data);
                    }
                }
                let _ = db_import.flush();
                
                let mut current_clients: Vec<_> = clients_model_import.iter().collect();
                current_clients.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                clients_model_import.set_vec(current_clients);
            }
        }
    });

    let db_search = db.clone();
    let clients_model_search = clients_model.clone();
    let active_timer_search = active_timer.clone();
    app.on_search_changed(move |query| {
        let query_str = query.to_string().to_lowercase();
        
        let mut new_clients = Vec::new();
        for res in db_search.iter() {
            if let Ok((k, v)) = res {
                let key_str = String::from_utf8_lossy(&k);
                if key_str.starts_with("client:") {
                    if let Ok(client) = serde_json::from_slice::<ClientDbData>(&v) {
                        if query_str.is_empty() || client.name.to_lowercase().contains(&query_str) {
                            let is_running = {
                                // Mantenere lo stato is_running se è attualmente attivo
                                let timer = active_timer_search.borrow();
                                if let Some((active_id, _)) = timer.as_ref() {
                                    *active_id == client.id
                                } else {
                                    false
                                }
                            };
                            
                            new_clients.push(ClientData {
                                id: client.id.into(),
                                name: client.name.into(),
                                last_duration_str: format_duration(client.last_duration_secs).into(),
                                is_running,
                            });
                        }
                    }
                }
            }
        }
        
        // Ordina alfabeticamente
        new_clients.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        
        // Aggiorna il modello rimpiazzando tutti gli elementi
        clients_model_search.set_vec(new_clients);
    });

    app.run().unwrap();

    Ok(())
}
