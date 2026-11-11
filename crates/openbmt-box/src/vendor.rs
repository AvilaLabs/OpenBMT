//! A POS vendor's reader for the Box (BX-49,
//! [rules](../../../docs/BOX-RULES.md) section 4).
//!
//! [`VendorConnector`] puts the readers of `openbmt-pos-readers` behind the
//! [`Connector`] trait: each poll builds the vendor's request for the cursor,
//! has [`HttpsRunner`] make it with the Box's POS token, and reads the reply
//! into one page (PS-80). The sync of `openbmt-pos` pages, stores the cursor
//! after each page and refuses a page that does not move it (PS-74). The
//! connector can close no order: Square is read-only (`docs/DESIGN.md`
//! section 5.6) and no Clover close is built, so [`Connector::can_close`] is
//! false and the Box closes nothing (BX-43).

use std::sync::Arc;

use openbmt_pos::{Connector, Cursor, Page, PosError, PosReader, Settings as PosSettings};
use openbmt_pos_readers::{clover, square};

use crate::https::{HttpsRefusal, HttpsRequest, HttpsRunner, Method, Transport};
use crate::pos_config::{PosConfig, PosConfigError, Vendor};

/// Orders asked for in one request (PS-80 allows 500).
pub const PAGE_LIMIT: usize = 100;
/// Seconds between two readings of a vendor's POS, and the least between two
/// (BX-49).
pub const VENDOR_POLL_SECS: u64 = 60;

/// The Square or Clover reader, over curl or a runner a test gives.
pub struct VendorConnector {
    vendor: Vendor,
    host: String,
    location: String,
    settings: PosSettings,
    socks: String,
    runner: Box<dyn HttpsRunner>,
    transport: Arc<Transport>,
    token: Option<String>,
}

impl VendorConnector {
    /// A connector for the vendor `config` names, with `runner` making the
    /// requests. `transport` says whether they go through Tor (BX-47) and
    /// keeps why the last one failed.
    pub fn new(
        config: &PosConfig,
        runner: Box<dyn HttpsRunner>,
        transport: Arc<Transport>,
    ) -> Result<Self, PosConfigError> {
        Ok(Self {
            vendor: config.vendor,
            host: config.host_url().to_string(),
            location: config.location.clone(),
            settings: config.settings(Vec::new())?,
            socks: config.socks_addr().to_string(),
            runner,
            transport,
            token: None,
        })
    }

    /// BX-45: the headers of every request; the token is in the first.
    fn headers(&self, token: &str, with_body: bool) -> Vec<(String, String)> {
        let mut headers = vec![
            ("Authorization".to_string(), format!("Bearer {token}")),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        if with_body {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }
        headers
    }

    fn request(&self, token: &str, cursor: &Cursor) -> Result<(HttpsRequest, Parse), HttpsRefusal> {
        let refused =
            |e: openbmt_pos_readers::RequestRefused| HttpsRefusal::BadRequest(e.to_string());
        let (mut request, parse) = match self.vendor {
            Vendor::Square => {
                let read =
                    square::Request::new(&self.settings, PAGE_LIMIT, cursor).map_err(refused)?;
                let mut request = HttpsRequest::new(
                    Method::Post,
                    format!("{}{}", self.host, square::SEARCH_PATH),
                );
                request.body = Some(read.body());
                request.headers = self.headers(token, true);
                (request, Parse::Square(read))
            }
            Vendor::Clover => {
                let read = clover::Request::new(&self.location, &self.settings, PAGE_LIMIT, cursor)
                    .map_err(refused)?;
                let mut request = HttpsRequest::new(
                    Method::Get,
                    format!("{}{}", self.host, read.path_and_query()),
                );
                request.headers = self.headers(token, false);
                (request, Parse::Clover(read))
            }
        };
        // BX-47: with Tor on the request goes through the proxy and through
        // nothing else.
        if self.transport.tor() {
            request.socks5 = Some(self.socks.clone());
        }
        Ok((request, parse))
    }

    fn page(&mut self, cursor: &Cursor) -> Result<Page, HttpsRefusal> {
        let Some(token) = self.token.clone() else {
            return Err(HttpsRefusal::TokenRefused { status: 401 });
        };
        let (request, parse) = self.request(&token, cursor)?;
        let body = self.runner.run(&request)?.into_body()?;
        let page = match parse {
            Parse::Square(read) => read.parse(&body, None),
            Parse::Clover(read) => read.parse(&body),
        };
        page.map_err(|e| HttpsRefusal::Unreadable(e.to_string()))
    }
}

enum Parse {
    Square(square::Request),
    Clover(clover::Request),
}

impl PosReader for VendorConnector {
    fn poll(&mut self, cursor: &Cursor) -> Result<Page, PosError> {
        self.transport.clear();
        self.page(cursor).map_err(|why| {
            let error = why.pos_error();
            self.transport.record(why);
            error
        })
    }
}

impl Connector for VendorConnector {
    fn set_token(&mut self, token: Option<&str>) {
        self.token = token.map(str::to_string);
    }
}
