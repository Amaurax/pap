use std::fs;
use axum::{Router, response::Html, routing::{get, post}, extract::{Form, State}};
use std::net::SocketAddr;
use tokio;
use tokio::net::TcpListener;
use reqwest;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use serde_json;
use axum::response::IntoResponse;
use htmlescape;

#[derive(Debug)]
struct Episode {
    title: String,
    date: String,
    description: String,
    image_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub chroniqueurs: Vec<String>,
    pub titre: String,
    pub lien: Option<String>,
    pub type_media: String, // film, livre, chaîne youtube/twitch, etc.
    pub description: String,
}

type RecommendationsStore = Arc<Mutex<HashMap<String, Vec<Recommendation>>>>;

#[derive(Deserialize)]
struct RecommendationForm {
    episode_title: String,
    chroniqueurs: String, // séparés par des virgules
    titre: String,
    lien: Option<String>,
    type_media: String,
    description: String,
}

async fn fetch_episodes() -> Vec<Episode> {
    let url = "https://feeds.acast.com/public/shows/portes-a-potes-pap";
    let xml = reqwest::get(url).await.unwrap().text().await.unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut buf = Vec::new();
    let mut episodes = Vec::new();
    let mut in_item = false;
    let mut title = String::new();
    let mut date = String::new();
    let mut description = String::new();
    let mut image_url = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"item" => {
                in_item = true;
                title.clear(); date.clear(); description.clear(); image_url.clear();
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"item" => {
                if in_item {
                    episodes.push(Episode {
                        title: title.clone(),
                        date: date.clone(),
                        description: description.clone(),
                        image_url: if image_url.is_empty() {
                            "https://via.placeholder.com/350x200?text=No+Image".to_string()
                        } else {
                            image_url.clone()
                        },
                    });
                }
                in_item = false;
            }
            Ok(Event::Start(ref e)) if in_item && e.name().as_ref() == b"title" => {
                title = reader.read_text(e.name()).unwrap_or_default().trim().to_string();
            }
            Ok(Event::Start(ref e)) if in_item && e.name().as_ref() == b"pubDate" => {
                date = reader.read_text(e.name()).unwrap_or_default().trim().to_string();
            }
            Ok(Event::Start(ref e)) if in_item && e.name().as_ref() == b"description" => {
                description = reader.read_text(e.name()).unwrap_or_default().trim().to_string();
            }
            Ok(Event::Empty(ref e)) if in_item && e.name().as_ref() == b"itunes:image" => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"href" {
                        image_url = attr.unescape_value().unwrap_or_default().trim().to_string();
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }
    episodes
}

async fn add_recommendation(
    State(store): State<RecommendationsStore>,
    Form(form): Form<RecommendationForm>,
) -> Html<String> {
    let mut map = store.lock().unwrap();
    let chroniqueurs: Vec<String> = form.chroniqueurs.split(',').map(|s| s.trim().to_string()).collect();
    let rec = Recommendation {
        chroniqueurs,
        titre: form.titre,
        lien: form.lien,
        type_media: form.type_media,
        description: form.description,
    };
    let episode_title = form.episode_title.clone();
    map.entry(episode_title.clone()).or_default().push(rec);
    save_recommendations(&map); // Sauvegarde après ajout
    let recos = map.get(&form.episode_title).unwrap();
    let last_reco = recos.last().unwrap();
    let chroniqueurs = last_reco.chroniqueurs.join(", ");
    let titre = if let Some(lien) = &last_reco.lien {
        format!("<a href='{}' target='_blank'>{}</a>", lien, last_reco.titre)
    } else {
        last_reco.titre.clone()
    };
    let html = format!(
        "<div class='reco'><div class='reco-header'><b>{}</b> <span class='reco-type'>[{}]</span></div><div class='reco-chroniqueurs'>{}</div><div class='reco-desc'>{}</div></div>",
        titre, last_reco.type_media, chroniqueurs, last_reco.description
    );
    Html(html)
}

async fn delete_recommendation(
    State(store): State<RecommendationsStore>,
    Form(params): Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let mut map = store.lock().unwrap();
    if let (Some(ep), Some(idx_str)) = (params.get("episode_title"), params.get("idx")) {
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let Some(list) = map.get_mut(ep) {
                if idx < list.len() {
                    list.remove(idx);
                    save_recommendations(&map);
                }
            }
        }
    }
    (axum::http::StatusCode::OK, "")
}

async fn episodes_page(State(store): State<RecommendationsStore>) -> Html<String> {
    let episodes = fetch_episodes().await;
    let map = store.lock().unwrap();
    // Options pour le select du modal global
    let mut episode_options = String::new();
    for ep in &episodes {
        episode_options.push_str(&format!("<option value=\"{}\">{}</option>", ep.title, ep.title));
    }
    let global_modal = format!(r#"
    <button id='open-global-reco-modal' class='add-reco-btn'>Ajouter une recommandation</button>
    <div class='modal-bg' id='global-reco-modal' style='display:none;visibility:hidden;' aria-hidden='true'>
        <div class='modal'>
            <button class='close-modal' title='Fermer'>&times;</button>
            <h3>Ajouter une recommandation</h3>
            <form class='reco-form' method='post' action='/add_reco' autocomplete='off'>
                <label for='episode_title'>Épisode concerné</label>
                <select id='episode_title' name='episode_title' required>{}</select>
                <label for='titre-global'>Titre</label>
                <input id='titre-global' name='titre' placeholder='Titre' required autocomplete='off'>
                <label for='lien-global'>Lien (optionnel)</label>
                <input id='lien-global' name='lien' placeholder='Lien' autocomplete='off'>
                <label for='chroniqueurs-global'>Chroniqueurs (séparés par des virgules)</label>
                <input id='chroniqueurs-global' name='chroniqueurs' placeholder='Chroniqueurs' required autocomplete='off'>
                <label for='type_media-global'>Type</label>
                <select id='type_media-global' name='type_media' required>
                    <option value='film'>Film</option>
                    <option value='livre'>Livre</option>
                    <option value='chaine youtube'>Chaîne YouTube</option>
                    <option value='chaine twitch'>Chaîne Twitch</option>
                    <option value='compte instagram'>Compte Instagram</option>
                    <option value='compte tiktok'>Compte TikTok</option>
                    <option value='musique'>Musique</option>
                    <option value='série'>Série</option>
                    <option value='jeu'>Jeu</option>
                    <option value='autre'>Autre</option>
                </select>
                <label for='description-global'>Description</label>
                <textarea id='description-global' name='description' placeholder='Description' required autocomplete='off'></textarea>
                <button type='submit'>Valider</button>
                <div class='reco-confirm' style='min-height:1.2em;'></div>
            </form>
        </div>
    </div>
    "#, episode_options);
    // Génération des cartes épisodes
    let mut cards = String::new();
    for ep in &episodes {
        if ep.title.trim().is_empty() {
            continue;
        }
        // Nettoie le titre pour affichage ET pour la clé de recherche dans le HashMap
        let raw_title = ep.title.replace("<![CDATA[", "").replace("]]>" , "").trim().to_string();
        let safe_title = htmlescape::encode_minimal(&raw_title);
        let safe_title_attr = safe_title.replace("\"", "&quot;").replace("'", "&#39;");
        // Nettoie la description (supprime CDATA, conserve le HTML)
        let desc = ep.description
            .replace("<![CDATA[", "")
            .replace("]]>" , "")
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .trim()
            .to_string();
        let img_tag = if ep.image_url.contains("placeholder.com") {
            String::new()
        } else {
            format!("<img class='episode-img' src='{}' alt='Image épisode' style='width:88px;height:88px;border-radius:14px;object-fit:contain;background:#fff;box-shadow:0 2px 8px #0002;' />", ep.image_url)
        };
        let data_ep = safe_title_attr.clone();
        // Utilise le titre nettoyé comme clé pour le HashMap
        let recos_html = map.get(&raw_title).map(|v| {
            v.iter().enumerate().map(|(idx, rec)| {
                let chroniqueurs = htmlescape::encode_minimal(&rec.chroniqueurs.join(", "));
                let titre = if let Some(lien) = &rec.lien {
                    format!("<a href=\"{}\" target=\"_blank\">{}</a>", htmlescape::encode_attribute(lien), htmlescape::encode_minimal(&rec.titre))
                } else {
                    htmlescape::encode_minimal(&rec.titre)
                };
                format!(
                    "<div class='reco-card' style='background:#f8f8f8;border-radius:10px;padding:0.8em 1em;margin-bottom:0.7em;box-shadow:0 1px 6px #0001;'>\
                        <div class='reco-header' style='display:flex;align-items:center;justify-content:space-between;'>\
                            <div><b>{titre}</b> <span class='reco-type' style='color:#b88;font-size:0.95em;'>[{type_media}]</span></div>\
                            <button class='delete-reco-btn' data-ep='{data_ep}' data-idx='{idx}' title='Supprimer' style='background:none;border:none;color:#c00;font-size:1.2em;cursor:pointer;'>&#10006;</button>\
                        </div>\
                        <div class='reco-chroniqueurs' style='color:#666;font-size:0.97em;margin-bottom:0.2em;'>{chroniqueurs}</div>\
                        <div class='reco-desc' style='font-size:0.98em;'>{description}</div>\
                    </div>",
                    idx=idx,
                    titre=titre,
                    type_media=htmlescape::encode_minimal(&rec.type_media),
                    data_ep=&data_ep,
                    chroniqueurs=chroniqueurs,
                    description=htmlescape::encode_minimal(&rec.description)
                )
            }).collect::<String>()
        }).unwrap_or_default();
        let show_recos_btn = if !recos_html.is_empty() {
            format!("<button class='show-recos-btn' data-ep='{}'>Voir les recommandations</button>", data_ep)
        } else {
            String::new()
        };
        let recos_block = if !recos_html.is_empty() {
            format!("<div class='recos' data-ep='{}' style='margin-top:1em'>{}</div>", data_ep, recos_html)
        } else {
            String::new()
        };
        let titre_affiche = if raw_title.trim().is_empty() { "Épisode sans titre".to_string() } else { safe_title.clone() };
        cards.push_str(&format!(
            "<div class='card'>
                <div class='card-top' style='display:flex;align-items:center;gap:1em;'>
                    <div class='img-col'>{img}</div>
                    <div class='info-col' style='flex:1;'>
                        <div style='font-weight:bold;font-size:1.1em'>{titre}</div>
                        <div class='date' style='color:#888;font-size:0.95em'>{date}</div>
                    </div>
                </div>
                <div class='desc' style='margin-top:0.7em'>{desc}</div>
                {show_recos_btn}
                {recos_block}
            </div>",
            img=img_tag,
            titre=titre_affiche,
            date=htmlescape::encode_minimal(&ep.date),
            desc=desc, // description HTML non échappée
            show_recos_btn=show_recos_btn,
            recos_block=recos_block
        ));
    }
    Html(format!(
        r#"
        <!DOCTYPE html>
        <html lang=\"fr\">
        <head>
            <meta charset=\"UTF-8\">
            <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">
            <title>Portes à Potes - Recommandations</title>
            <link rel=\"stylesheet\" href=\"/static/styles.css\">
            <script src=\"/static/scripts.js\" defer></script>
            <script>
                document.addEventListener('DOMContentLoaded', function() {{
                    var openBtn = document.getElementById('open-global-reco-modal');
                    var modalBg = document.getElementById('global-reco-modal');
                    var closeBtn = modalBg ? modalBg.querySelector('.close-modal') : null;
                    if (openBtn && modalBg) {{
                        openBtn.addEventListener('click', function() {{
                            modalBg.style.display = 'flex';
                            modalBg.style.visibility = 'visible';
                            modalBg.setAttribute('aria-hidden', 'false');
                        }});
                    }}
                    if (closeBtn && modalBg) {{
                        closeBtn.addEventListener('click', function() {{
                            modalBg.style.display = 'none';
                            modalBg.style.visibility = 'hidden';
                            modalBg.setAttribute('aria-hidden', 'true');
                        }});
                    }}
                    // Fermer le modal si on clique sur le fond
                    if (modalBg) {{
                        modalBg.addEventListener('click', function(e) {{
                            if (e.target === modalBg) {{
                                modalBg.style.display = 'none';
                                modalBg.style.visibility = 'hidden';
                                modalBg.setAttribute('aria-hidden', 'true');
                            }}
                        }});
                    }}
                }});
            </script>
            <style>
                body {{
                    min-height: 100vh;
                    margin: 0;
                    padding: 0;
                    position: relative;
                    overflow-x: hidden;
                }}
                .background-blur {{
                    position: fixed;
                    top: 0; left: 0; right: 0; bottom: 0;
                    z-index: 0;
                    background: url('https://assets.pippa.io/shows/cover/1678196289243-eb4dc05a818625489cad37a30940fd3b.jpeg') center center/cover no-repeat;
                    filter: blur(18px) brightness(0.7);
                    width: 100vw;
                    height: 100vh;
                }}
                .main-content {{
                    position: relative;
                    z-index: 1;
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    min-height: 100vh;
                }}
                .podslink-widget-container {{
                    width: 100%;
                    max-width: 600px;
                    margin: 0 auto 2em auto;
                    position: relative;
                    z-index: 5;
                    background: rgba(30,30,30,0.85);
                    border-radius: 18px;
                    box-shadow: 0 2px 16px #0004;
                    padding: 1.2em 0.5em 0.7em 0.5em;
                    display: flex;
                    justify-content: center;
                }}
                .podslink-widget-container iframe {{
                    border-radius: 12px;
                    min-height: 120px;
                    background: transparent;
                }}
                .card {{
                    margin-left: auto;
                    margin-right: auto;
                    margin-bottom: 2em;
                    background: rgba(255,255,255,0.92);
                    border-radius: 18px;
                    box-shadow: 0 2px 16px #0002;
                    padding: 1.2em 1.5em;
                    max-width: 700px;
                    width: 100%;
                }}
                .add-reco-btn {{
                    display: inline-block;
                    background: linear-gradient(90deg,#ffb347,#ffcc33);
                    color: #222;
                    font-weight: bold;
                    font-size: 1.15em;
                    border: none;
                    border-radius: 2em;
                    padding: 0.7em 2.2em;
                    margin: 2em 0 2.5em 0;
                    box-shadow: 0 2px 8px #0001;
                    cursor: pointer;
                    transition: background 0.2s, box-shadow 0.2s;
                }}
                .add-reco-btn:hover {{
                    background: linear-gradient(90deg,#ffe082,#ffd54f);
                    box-shadow: 0 4px 16px #0002;
                }}
                .modal-bg {{
                    position: fixed;
                    top: 0; left: 0; right: 0; bottom: 0;
                    background: rgba(0,0,0,0.35);
                    z-index: 10;
                    display: flex;
                    align-items: center;
                    justify-content: center;
                }}
                .modal {{
                    background: #fff;
                    border-radius: 18px;
                    box-shadow: 0 4px 32px #0003;
                    padding: 2.2em 2.5em 2em 2.5em;
                    min-width: 320px;
                    max-width: 95vw;
                    position: relative;
                    animation: modalIn 0.2s;
                }}
                @keyframes modalIn {{
                    from {{ transform: scale(0.95); opacity: 0; }}
                    to {{ transform: scale(1); opacity: 1; }}
                }}
                .close-modal {{
                    position: absolute;
                    top: 1.1em;
                    right: 1.3em;
                    background: none;
                    border: none;
                    font-size: 1.7em;
                    color: #888;
                    cursor: pointer;
                }}
                .reco-form label {{
                    display: block;
                    margin-top: 1.1em;
                    margin-bottom: 0.3em;
                    font-weight: 500;
                }}
                .reco-form input, .reco-form select, .reco-form textarea {{
                    width: 100%;
                    padding: 0.6em;
                    border-radius: 8px;
                    border: 1px solid #ccc;
                    font-size: 1em;
                    margin-bottom: 0.2em;
                    background: #fafafa;
                }}
                .reco-form textarea {{
                    min-height: 70px;
                    resize: vertical;
                }}
                .reco-form button[type='submit'] {{
                    margin-top: 1.3em;
                    background: linear-gradient(90deg,#ffb347,#ffcc33);
                    color: #222;
                    font-weight: bold;
                    border: none;
                    border-radius: 2em;
                    padding: 0.7em 2.2em;
                    font-size: 1.1em;
                    cursor: pointer;
                    box-shadow: 0 2px 8px #0001;
                    transition: background 0.2s, box-shadow 0.2s;
                }}
                .reco-form button[type='submit']:hover {{
                    background: linear-gradient(90deg,#ffe082,#ffd54f);
                    box-shadow: 0 4px 16px #0002;
                }}
                .listen-links {{
                    max-width: 600px;
                    margin: 0 auto 2em auto;
                    display: flex;
                    flex-wrap: wrap;
                    gap: 1em;
                    justify-content: center;
                    align-items: center;
                }}
                .listen-btn {{
                    width: 3.2em;
                    height: 3.2em;
                    border-radius: 50%;
                    display: flex;
                    align-items: center;
                    justify-content: center;
                    background: #fff;
                    border: 1px solid #eee;
                    box-shadow: 0 2px 8px #0001;
                    color: #222;
                    text-decoration: none;
                    transition: box-shadow 0.2s, background 0.2s, color 0.2s;
                    margin: 0;
                    padding: 0;
                }}
                .listen-btn:hover {{
                    box-shadow: 0 4px 16px #0002;
                    background: #f0f0f0;
                    color: #111;
                }}
                .listen-btn.apple {{ background: linear-gradient(135deg,#a259ff,#f6c3ff); border:none; color:#fff; }}
                .listen-btn.apple:hover {{ background: linear-gradient(135deg,#c299fc,#f6e3ff); color:#fff; }}
                .listen-btn.spotify {{ background: #1db954; border:none; color:#fff; }}
                .listen-btn.spotify:hover {{ background: #17a34a; color:#fff; }}
                .listen-btn.deezer {{ background: linear-gradient(135deg,#232526,#414345); border:none; color:#fff; }}
                .listen-btn.deezer:hover {{ filter: brightness(1.1); color:#fff; }}
                .listen-btn.acast {{ background: linear-gradient(135deg,#ff9800,#ffb347); border:none; color:#fff; }}
                .listen-btn.acast:hover {{ background: linear-gradient(135deg,#ffc266,#ffe0b2); color:#b85c00; }}
                .listen-logo {{
                    width: 1.7em;
                    height: 1.7em;
                    object-fit: contain;
                    display: block;
                    margin: 0;
                }}
            </style>
        </head>
        <body>
            <div class='background-blur'></div>
            <div class='main-content'>
                <header>
                    <h1>Portes à Potes</h1>
                </header>
                <div class="listen-links" style="max-width:600px;margin:0 auto 2em auto;display:flex;flex-wrap:wrap;gap:1em;justify-content:center;align-items:center;">
                    <a href="https://podcasts.apple.com/fr/podcast/portes-%C3%A0-potes/id1676606425" target="_blank" rel="noopener" class="listen-btn apple" title="Apple Podcasts">
                        <img src="https://upload.wikimedia.org/wikipedia/commons/thumb/e/e7/Podcasts_%28iOS%29.svg/300px-Podcasts_%28iOS%29.svg.png" alt="Apple Podcasts" class="listen-logo">
                    </a>
                    <a href="https://open.spotify.com/show/08mBuJPR173kee3Hj500ol?si=b40b642320db45af" target="_blank" rel="noopener" class="listen-btn spotify" title="Spotify">
                        <img src="https://upload.wikimedia.org/wikipedia/commons/thumb/8/84/Spotify_icon.svg/512px-Spotify_icon.svg.png?20220821125323" alt="Spotify" class="listen-logo">
                    </a>
                    <a href="https://dzr.page.link/KSSbPUMwubqNqzgc6" target="_blank" rel="noopener" class="listen-btn deezer" title="Deezer">
                        <img src="https://companieslogo.com/img/orig/DEEZR.PA-dbdcf2cf.png?t=1721547851" alt="Deezer" class="listen-logo">
                    </a>
                    <a href="https://feeds.acast.com/public/shows/portes-a-potes-pap" target="_blank" rel="noopener" class="listen-btn acast" title="Acast RSS">
                        <img src="https://upload.wikimedia.org/wikipedia/commons/thumb/4/46/Generic_Feed-icon.svg/256px-Generic_Feed-icon.svg.png?20120905025810" alt="RSS" class="listen-logo">
                    </a>
                </div>
                <div style='display:flex;justify-content:center;'>
                    {global_modal}
                </div>
                <main style='width:100%;max-width:900px;'>
                    {cards}
                </main>
                <footer>
                    <p>&copy; 2023 Portes à Potes. Tous droits réservés.</p>
                </footer>
            </div>
        </body>
        </html>
        "#,
        global_modal=global_modal,
        cards=cards
    ))
}

fn save_recommendations(map: &HashMap<String, Vec<Recommendation>>) {
    let json = serde_json::to_string_pretty(map).unwrap();
    let _ = fs::write("recommandations.json", json);
}

fn load_recommendations() -> RecommendationsStore {
    let mut store: RecommendationsStore = Arc::new(Mutex::new(HashMap::new()));
    if let Ok(json) = fs::read_to_string("recommandations.json") {
        let map: HashMap<String, Vec<Recommendation>> = serde_json::from_str(&json).unwrap_or_default();
        store = Arc::new(Mutex::new(map));
    }
    store
}

#[tokio::main]
async fn main() {
    let store: RecommendationsStore = load_recommendations();
    let app = Router::new()
        .route("/", get(episodes_page))
        .route("/add_reco", post(add_recommendation))
        .route("/delete_reco", post(delete_recommendation))
        .with_state(store.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Serveur lancé sur http://{}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
