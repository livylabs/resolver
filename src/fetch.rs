use crate::errors::FetchError;
use serde_json::Value;
use spider_client::{Spider, RequestParams,ReturnFormatHandling,ReturnFormat,RequestType, WaitFor, IdleNetwork, Timeout};
use std::time::Duration;


pub struct Fetcher {
    spider: Spider
}

impl Fetcher {

    pub fn new() -> Self {
        dotenvy::dotenv().ok();
        let key = std::env::var("SPIDER_KEY").unwrap();
        let spider  = Spider::new(Some(key)).expect("Can initiate fetcher service");
        Fetcher { spider}
    }
    fn default_params() -> RequestParams {
        RequestParams {
            return_format: Some(ReturnFormatHandling::Single(ReturnFormat::Markdown)),
            request: Some(RequestType::SmartMode),
            ..Default::default() 
        }
    } 
    pub async fn get_data(&self , source:&str ,params: Option<RequestParams>) -> Result< serde_json::Value,FetchError> {
        //Upgrade this for prod
      //  let params = params.unwrap_or_else(Self::default_params);
        let params = RequestParams {
            return_format: Some(ReturnFormatHandling::Single(ReturnFormat::Markdown)),
            request: Some(RequestType::Chrome),
            stealth: Some(true),
            proxy_enabled: Some(true),
            ..Default::default() 
        };

        let crawl = self.spider.scrape_url(source, Some(params),"aplication/json").await.map_err(FetchError::UnableFetch)?;
        let crawl = match crawl {
            Value::String(s) => serde_json::from_str::<Value>(&s)?,
            other => other,  
        };
        Ok(crawl)
    }

    pub async fn unblocker(&self , source: &str ) -> Result<serde_json::Value , FetchError>{
let params = RequestParams {
            return_format: Some(ReturnFormatHandling::Single(ReturnFormat::Markdown)),
            request: Some(RequestType::Chrome),
            stealth: Some(true),
            fingerprint: Some(true),
            proxy_enabled: Some(true),
            ..Default::default()
        };
        let data =  self.spider.unblock_url(source, Some(params) , "aplication/json").await.map_err(FetchError::UnableFetch)?;
        let crawl = match data {
            Value::String(s) => serde_json::from_str::<Value>(&s)?,
            other => other,  
        };
        Ok(crawl)
}

    }


