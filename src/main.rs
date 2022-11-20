use std::collections::{HashMap, BTreeMap};
use serde::Deserialize;
use serde::Serialize;
use std::cmp::Ordering;
use std::future::Future;
use reqwest::Client;
use futures::StreamExt;
use std::sync::{Arc, Mutex};
use std::str::FromStr;
use num_format::{Locale,ToFormattedString};
use std::io::Write;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let mut settings_path = std::env::current_dir().unwrap().join("settings.json");

    let mut settings = Settings::default();

    if !settings_path.exists() {
        println!("Settings file missing...");
        let json = serde_json::to_string(&settings).unwrap();
        std::fs::write(&settings_path,json);
        println!("Settings file created!")
    } else {
        let json = std::fs::read_to_string(&settings_path).unwrap();
        settings = serde_json::from_str(json.as_str()).unwrap();
    }

    let api_key = "eaceeee6-6a3f-4ae6-bb33-9cfee0f2e6fc";
    let mut hypixel_api = HypixelAPI::new(api_key.to_string());
    let mut library = Arc::new(Mutex::new(AuctionLibrary::new()));
    set_window_name(&settings);
    request_pages(&hypixel_api,library.clone(),&settings.reforges,settings.price,settings.min_margin,settings.remove_pets,settings.remove_dungeon).await;

    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input);
        input.trim();

        if input.contains("price") {
            let mut pr = input.split(" ").collect::<Vec<&str>>()[1].trim().to_string();
            if pr.contains(".") {
                pr = pr.replace(".","");
            }
            let price = pr.parse::<i32>().unwrap();
            settings.price = price;
            set_window_name(&settings);
            save_settings(&settings_path,&settings);
        }else if input.contains("min_margin") {
            let mut mm = input.split(" ").collect::<Vec<&str>>()[1].trim().to_string();

            if mm.contains(".") {
                mm = mm.replace(".","");
            }

            let min_margin = mm.parse::<i32>().unwrap();
            settings.min_margin = min_margin;
            set_window_name(&settings);
            save_settings(&settings_path,&settings);
        } else if input.contains("remove_pets") {
            let mm = input.split(" ").collect::<Vec<&str>>()[1].trim();
            let rp = mm.parse::<bool>().expect("Not a value! (true/false)");
            settings.remove_pets = rp;
            set_window_name(&settings);
            save_settings(&settings_path,&settings);
        }  else if input.contains("remove_dungeon") {
            let mm = input.split(" ").collect::<Vec<&str>>()[1].trim();
            let rd = mm.parse::<bool>().expect("Not a value! (true/false)");
            settings.remove_dungeon = rd;
            set_window_name(&settings);
            save_settings(&settings_path,&settings);
        } else {
            println!("RELOADING!");
            library.lock().unwrap().clear();
            request_pages(&hypixel_api,library.clone(),&settings.reforges,settings.price,settings.min_margin,settings.remove_pets,settings.remove_dungeon).await;
        }
    }
}

pub fn set_window_name(settings:&Settings) {
    winconsole::console::set_title(format!("HypixelAuctionHelper (price={}) (min_margin={}) (remove_pets={}) (remove_dungeon={})",settings.price.to_formatted_string(&Locale::de),settings.min_margin.to_formatted_string(&Locale::de),settings.remove_pets.to_string(),settings.remove_dungeon.to_string()).as_str());
}

pub fn save_settings(settings_path:&PathBuf,settings:&Settings) {
    let json = serde_json::to_string(&settings).unwrap();
    std::fs::write(settings_path,json);
}

async fn request_pages(hypixel_api:&HypixelAPI,library:Arc<Mutex<AuctionLibrary>>,reforges:&Vec<String>,price:i32,min_margin:i32,rp:bool,rd:bool) {
    winconsole::console::clear();
    let first_reply = hypixel_api.get_skyblock_auctions(0).await.unwrap();

    let pages = first_reply.totalPages;

    let mut urls = vec![];

    for i in 0..pages {
        let mut params : HashMap<String,String> = HashMap::new();
        params.insert("page".to_string(), i.to_string());
        urls.push(hypixel_api.create_request("skyblock/auctions",params));
    }

    let client = Client::new();

    let mut finished = Arc::new(Mutex::new(0));

    let bodies = futures::stream::iter(urls).map(|url| {
        let cl = &client;
        let refs = &reforges;
        let l = library.clone();
        let f = finished.clone();

        async move {
            let resp = cl.get(url).send().await.unwrap();
            let text = resp.text().await.unwrap();

            let reply_opt = serde_json::from_str::<SkyBlockAuctionsReply>(text.as_str());

            if reply_opt.is_ok() {
                let reply = reply_opt.unwrap();
                for item in reply.auctions {
                    if item.bin.is_some() && !item.is_bad_reforge(refs) {
                        if item.bin.unwrap() {
                            l.lock().unwrap().add(item);
                        }
                    }
                }
                *f.lock().unwrap() += 1;

                print!("\r");
                print!("{}/{}",pages,*f.lock().unwrap());
                std::io::stdout().flush().unwrap();
            }
        }
    }).buffer_unordered(10);

    bodies.for_each(|b|{
        async {

        }
    }).await;

    println!("\n");

    let mut lock = library.lock().unwrap();
    {
        println!("FINISHED!");
        lock.finish();

        let map = lock.get_sorted(20,price,min_margin,rp,rd);
        for id in map.keys() {
            println!("{} {} {}",(*id).to_formatted_string(&Locale::de),map[&id][0].item_name,map[&id][0].starting_bid.to_formatted_string(&Locale::de));
        }
    }
}

struct HypixelAPI {
    base_url: String,
    api_key: String
}

impl HypixelAPI {
    pub fn new(api_key:String) -> Self {
        return Self { base_url: "https://api.hypixel.net/".to_string(), api_key }
    }

    pub async fn get_skyblock_auctions(&self, page: i32) -> Option<SkyBlockAuctionsReply>{
        let mut params : HashMap<String,String> = HashMap::new();
        params.insert("page".to_string(), page.to_string());
        let path = self.create_request("skyblock/auctions",params);

        let result = reqwest::get(path.clone()).await;
        if result.is_ok() {
            let response = result.unwrap();

            let text_result = response.text().await;

            if text_result.is_ok() {
                let text = text_result.unwrap();

                let reply_opt = serde_json::from_str::<SkyBlockAuctionsReply>(text.as_str());

                if reply_opt.is_ok() {
                    return Some(reply_opt.unwrap());
                }
            }
        }

        None
    }

    pub fn create_request(&self,path:&str,params:HashMap<String,String>) -> String{
        let mut request_path = format!("{}{}",self.base_url,path);

        request_path.push_str(&format!("?key={}",self.api_key));

        for (key,obj) in params.iter() {
            request_path.push_str(&format!("&{}={}",key,obj));
        }

        return request_path;
    }
}

#[derive(Serialize, Deserialize)]
struct SkyBlockAuctionsReply {
    pub success: bool,
    pub page:i32,
    pub totalPages:i32,
    pub totalAuctions:i32,
    pub lastUpdated:i64,
    pub auctions: Vec<AuctionItem>
}

impl SkyBlockAuctionsReply {
    pub fn has_next_page(&self) -> bool {
        return self.page < self.totalPages - 1
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq)]
struct AuctionItem {
    pub uuid:String,
    pub item_name:String,
    pub starting_bid:i32,
    pub tier: String,
    pub bin: Option<bool>
}

impl AuctionItem {
    pub fn is_bad_reforge(&self,reforges:&Vec<String>) -> bool {
        for reforge in reforges.iter() {
            if self.item_name.contains(reforge) {
                return true;
            }
        }

        return false;
    }
}

struct AuctionLibrary {
    auction_items:HashMap<String,Vec<AuctionItem>>
}

impl AuctionLibrary {
    pub fn new() -> AuctionLibrary {
        return AuctionLibrary { auction_items: HashMap::new() }
    }

    pub fn add(&mut self,mut item:AuctionItem) {
        if !self.auction_items.contains_key(&item.item_name) {
            self.auction_items.insert(item.item_name.clone(),Vec::new());
        }
        self.auction_items.get_mut(&item.item_name).unwrap().push(item);
    }

    pub fn finish(&mut self) {
        for (key,items) in self.auction_items.iter_mut() {
            items.sort_by(|a,b| { a.starting_bid.cmp(&b.starting_bid)});
        }
    }

    pub fn get_highest_margin(name:String,items:&Vec<AuctionItem>,demand:i32,price:i32) -> i32{

        let mut highest_margin = 0;

        if items.len() > 1 && items.len() >= demand as usize {
            let margin = items[1].starting_bid - items[0].starting_bid;
            if margin > highest_margin {
                let higher = items[1].starting_bid;
                let lower = items[0].starting_bid;
                if lower <= price {
                    highest_margin = higher - lower
                }
            }
        }

        return highest_margin;
    }

    pub fn get_sorted(&self,demand:i32,price:i32,min_margin:i32,remove_pets:bool,remove_dungeon:bool) -> BTreeMap<i32, &Vec<AuctionItem>> {
        let mut map : BTreeMap<i32, &Vec<AuctionItem>> = BTreeMap::new();

        for (id,items) in self.auction_items.iter() {
            let pet = id.contains("Lvl");
            let dun = id.contains("✪");

            if remove_pets && pet {
                continue;
            }

            if remove_dungeon && dun {
                continue
            }

            let margin = AuctionLibrary::get_highest_margin(id.clone(),items,demand,price);
            if margin >= min_margin {
                map.insert(margin,items);
            }
        }

        return map;
    }

    pub fn clear(&mut self) {
        self.auction_items.clear();
    }
}

#[derive(Serialize, Deserialize)]
pub struct Settings {
    pub price:i32,
    pub min_margin:i32,
    pub remove_pets:bool,
    pub remove_dungeon:bool,
    pub reforges:Vec<String>
}

impl Settings {
    pub fn default() -> Settings {
        return Settings {
            price: 10000000,
            min_margin: 100000,
            remove_pets: true,
            remove_dungeon: true,
            reforges: vec!["Ancient".to_string(),"Fierce".to_string(),"Necrotic".to_string(),"Sharp".to_string(),"Legendary".to_string(),"Godly".to_string(),"Spiritual".to_string(),"Heroic".to_string(),"Spicy".to_string(),"Wise".to_string(),"Fleet".to_string(),"Unreal".to_string(),"Rapid".to_string(),"Fabled".to_string(),"Submerged".to_string(),"Treacherous".to_string(),"Skin".to_string(),"Withered".to_string()]
        }
    }
}
