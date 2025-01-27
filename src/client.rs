/*!
module for creating clients with various authentication methods

Each client has the type `Client<A: Authenticate>`.
You can create a client with the functions provided by this module.

# Examples
```rust
use powerplatform_dataverse_service_client::client::Client;

let client_id = "<clientid>";
let client_secret = "<clientsecret>";

let client = Client::with_client_secret_auth(
    "https://instance.crm.dynamics.com/",
    "12345678-1234-1234-1234-123456789012",
    client_id,
    client_secret,
);
```
*/

use std::future::Future;
use std::{borrow::Cow, fmt::Display};
use std::time::Duration;

use lazy_static::lazy_static;
use regex::Regex;
use reqwest::{RequestBuilder, Response, Method};
use serde::Deserialize;
use uuid::Uuid;

use crate::action::MergeRequest;
use crate::{
    auth::{client_secret::ClientSecretAuth, Authenticate, no_auth::NoAuth},
    batch::Batch,
    entity::{ReadEntity, WriteEntity},
    error::DataverseError,
    query::Query,
    reference::Reference,
    result::{IntoDataverseResult, Result},
};

lazy_static! {
    static ref UUID_REGEX: Regex =
        Regex::new("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
            .unwrap();
}

/// Microsoft Dataverse Web-API Version this client uses
pub static VERSION: &str = "9.2";
/**
A client capable of connecting to a dataverse environment

A client should be created once and then reused to take advantage of its
connection-pooling.

# Examples
```rust
use powerplatform_dataverse_service_client::client::Client;

let client_id = "<clientid>";
let client_secret ="<clientsecret>";

let client = Client::with_client_secret_auth(
    "https://instance.crm.dynamics.com/",
    "12345678-1234-1234-1234-123456789012",
    client_id,
    client_secret,
);
```
*/
pub struct Client<'url, A: Authenticate> {
    pub url: Cow<'url, str>,
    backend: reqwest::Client,
    auth: A,
}

impl<'url> Client<'url, ClientSecretAuth> {
    /**
    Creates a dataverse client that uses client/secret authentication

    Please note that this function will not fail right away even when the
    provided credentials are invalid. This is because the authentication
    is handled lazily and a token is only acquired on the first call or
    when an acquired token is no longer valid and needs to be refreshed

    # Examples
    ```rust
    use powerplatform_dataverse_service_client::client::Client;

    let client_id = "<clientid>";
    let client_secret = "<clientsecret>";

    let client = Client::with_client_secret_auth(
        "https://instance.crm.dynamics.com/",
        "12345678-1234-1234-1234-123456789012",
        client_id,
        client_secret,
    );
    ```
    */
    pub fn with_client_secret_auth(
        url: impl Into<Cow<'url, str>>,
        tenant_id: &str,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        let url = url.into();
        let client_id = client_id.into();
        let client_secret = client_secret.into();
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(120))
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap();

        let auth = ClientSecretAuth::new(
            client.clone(),
            format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                tenant_id
            ),
            format!("{}.default", url),
            client_id,
            client_secret,
        );

        Client::new(url, client, auth)
    }
}

impl<'url> Client<'url, NoAuth> {
    /**
    Creates a dummy Client that will return errors every time its functions are used

    This is only really useful in unit-testing and doc-testing scenarios where you
    want to prevent a bunch of erronous auth-calls each time a test is run
    */
    pub fn new_dummy() -> Self {
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(120))
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap();

        let auth = NoAuth {};
        Client::new("", client, auth)
    }
}

impl<'url, A: Authenticate> Client<'url, A> {
    /**
    Creates a dataverse client with a custom authentication handler and backend

    This function may not panic so the custom authentication should follow these
    rules:
    - tokens should be acquired lazily
    - tokens should be cached and reused where possible
    - each call to the `get_valid_token()` function should give a token that is valid
    for at least the next 2 minutes

    # Examples
    ```rust
    use core::time::Duration;
    use powerplatform_dataverse_service_client::auth::client_secret::ClientSecretAuth;
    use powerplatform_dataverse_service_client::client::Client;
    use powerplatform_dataverse_service_client::result::{IntoDataverseResult, Result};

    # fn main() -> Result<()> {
    let tenant_id = "12345678-1234-1234-1234-123456789012";
    let client_id = String::from("<some client id>");
    let client_secret = String::from("<some client secret>");
    let url = "https://instance.crm.dynamics.crm/";

    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(120))
        .timeout(Duration::from_secs(120))
        .build().into_dataverse_result()?;

    let auth = ClientSecretAuth::new(
        client.clone(),
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant_id
        ),
        format!("{}.default", url),
        client_id,
        client_secret,
    );

    let client = Client::new(url, client, auth);
    # Ok(())
    # }
    ```
    */
    pub fn new(url: impl Into<Cow<'url, str>>, backend: reqwest::Client, auth: A) -> Self {
        let url = url.into();
        Self { url, backend, auth }
    }

    /**
    Writes the given entity into the current dataverse instance and returns its generated Uuid

    This may fail for any of these reasons
    - An authentication failure
    - A serde serialization error
    - Any http client or server error
    - there is already a record with this Uuid in the table

    # Examples
    ```rust
    use uuid::Uuid;
    use serde::Serialize;
    use powerplatform_dataverse_service_client::client::Client;
    use powerplatform_dataverse_service_client::entity::WriteEntity;
    use powerplatform_dataverse_service_client::reference::{Reference, ReferenceStruct};
    use powerplatform_dataverse_service_client::result::{IntoDataverseResult, Result};

    async fn test() -> Result<Uuid> {
        let contact = Contact {
            contactid: Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?,
            firstname: String::from("Testy"),
            lastname: String::from("McTestface"),
        };

        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        client.create(&contact).await
    }

    #[derive(Serialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl WriteEntity for Contact {}

    impl Reference for Contact {
        fn get_reference(&self) -> ReferenceStruct {
            ReferenceStruct::new(
                "contacts",
                self.contactid,
            )
        }
    }
    ```
    */
    pub async fn create(&self, entity: &impl WriteEntity) -> Result<Uuid> {
        let reference = entity.get_reference();
        let url_path = self.build_simple_url(reference.entity_name);

        async fn handle_response(response: Response) -> Result<Uuid> {
            if response.status().is_client_error() || response.status().is_server_error() {
                let error_message = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("no error details provided from server"));
                return Err(DataverseError::new(error_message));
            }
    
            let header_value = response
                .headers()
                .get("OData-EntityId")
                .ok_or_else(|| DataverseError::new("Dataverse provided no Uuid".to_string()))?;
    
            let uuid_segment = UUID_REGEX
                .find(header_value.to_str().unwrap_or(""))
                .ok_or_else(|| DataverseError::new("Dataverse provided no Uuid".to_string()))?;
    
            Uuid::parse_str(uuid_segment.as_str()).into_dataverse_result()
        }

        self.request(
            Method::POST, 
            &url_path, 
            move |request| {
                Ok(request
                    .header("Content-Type", "application/json")
                    .body(serde_json::to_vec(entity).into_dataverse_result()?)
                )
            }, 
            handle_response
        ).await
    }

    /**
    Updates the attributes of the gven entity in the current dataverse instance

    Please note that only those attributes are updated that are present in the
    serialization payload. Other attributes are untouched

    This may fail for any of these reasons
    - An authentication failure
    - A serde serialization error
    - Any http client or server error
    - there is no record with this Uuid in the table

    # Examples
    ```rust
    use uuid::Uuid;
    use serde::Serialize;
    use powerplatform_dataverse_service_client::client::Client;
    use powerplatform_dataverse_service_client::entity::WriteEntity;
    use powerplatform_dataverse_service_client::reference::{Reference, ReferenceStruct};
    use powerplatform_dataverse_service_client::result::{IntoDataverseResult, Result};

    async fn test() -> Result<()> {
        let contact = Contact {
            contactid: Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?,
            firstname: String::from("Testy"),
            lastname: String::from("McTestface"),
        };

        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        client.update(&contact).await
    }

    #[derive(Serialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl WriteEntity for Contact {}

    impl Reference for Contact {
        fn get_reference(&self) -> ReferenceStruct {
            ReferenceStruct::new(
                "contacts",
                self.contactid,
            )
        }
    }
    ```
    */
    pub async fn update(&self, entity: &impl WriteEntity) -> Result<()> {
        let reference = entity.get_reference();
        let url_path = self.build_targeted_url(reference.entity_name, reference.entity_id);

        self.request(
            Method::PATCH,
            &url_path, 
            move |request| {
                Ok(request
                    .header("Content-Type", "application/json")
                    .header("If-Match", "*")
                    .body(serde_json::to_vec(entity).into_dataverse_result()?)
                )
            }, 
            handle_empty_response
        ).await
    }

    /**
    Updates or creates the given entity in the current dataverse instance

    Please note that only those attributes are updated that are present in the
    serialization payload. Other attributes are untouched

    This may fail for any of these reasons
    - An authentication failure
    - A serde serialization error
    - Any http client or server error

    # Examples
    ```rust
    use uuid::Uuid;
    use serde::Serialize;
    use powerplatform_dataverse_service_client::client::Client;
    use powerplatform_dataverse_service_client::entity::WriteEntity;
    use powerplatform_dataverse_service_client::reference::{Reference, ReferenceStruct};
    use powerplatform_dataverse_service_client::result::{IntoDataverseResult, Result};

    async fn test() -> Result<()> {
        let contact = Contact {
            contactid: Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?,
            firstname: String::from("Testy"),
            lastname: String::from("McTestface"),
        };

        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        client.upsert(&contact).await
    }

    #[derive(Serialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl WriteEntity for Contact {}

    impl Reference for Contact {
        fn get_reference(&self) -> ReferenceStruct {
            ReferenceStruct::new(
                "contacts",
                self.contactid,
            )
        }
    }
    ```
    */
    pub async fn upsert(&self, entity: &impl WriteEntity) -> Result<()> {
        let reference = entity.get_reference();
        let url_path = self.build_targeted_url(reference.entity_name, reference.entity_id);

        self.request(
            Method::PATCH, 
            &url_path, 
            move |request| {
                Ok(request
                    .header("Content-Type", "application/json")
                    .body(serde_json::to_vec(entity).into_dataverse_result()?)
                )
            }, 
            handle_empty_response
        ).await
    }

    /**
    Deletes the entity record this reference points to

    Please note that each structs that implements `WriteEntity` also implements
    `Reference` so you can use it as input here, but there is a sensible default implementation
    called `ReferenceStruct` for those cases where you only have access to the raw
    reference data

    This may fail for any of these reasons
    - An authentication failure
    - Any http client or server error
    - The referenced entity record doesn't exist

    # Examples
    ```rust
    use uuid::Uuid;
    use powerplatform_dataverse_service_client::client::Client;
    use powerplatform_dataverse_service_client::reference::ReferenceStruct;
    use powerplatform_dataverse_service_client::result::{IntoDataverseResult, Result};

    # async fn test() -> Result<()> {
    let reference = ReferenceStruct::new(
        "contacts",
        Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?
    );

    let client = Client::new_dummy(); // Please replace this with your preferred authentication method
    client.delete(&reference).await?;
    # Ok(())
    # }
    ```
    */
    pub async fn delete(&self, reference: &impl Reference) -> Result<()> {
        let reference = reference.get_reference();
        let url_path = self.build_targeted_url(reference.entity_name, reference.entity_id);

        self.request(
            Method::DELETE, 
            &url_path, 
            move |request| Ok(request), 
            handle_empty_response
        ).await
    }

    /**
    retrieves the entity record that the reference points to from dataverse

    This function uses the implementation of the `Select` trait to only retrieve
    those attributes relevant to the struct defined. It is an Anti-Pattern to
    retrieve all attributes when they are not needed, so this library does not
    give the option to do that

    This may fail for any of these reasons
    - An authentication failure
    - A serde deserialization error
    - Any http client or server error
    - The entity record referenced doesn't exist

    # Examples
    ```rust
    use serde::Deserialize;
    use uuid::Uuid;
    use powerplatform_dataverse_service_client::{
        client::Client,
        entity::ReadEntity,
        reference::ReferenceStruct,
        result::{IntoDataverseResult, Result},
        select::Select
    };
    
    async fn test() -> Result<()> {
        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        let contact: Contact = client
            .retrieve(
                &ReferenceStruct::new(
                    "contacts",
                    Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?
                )
            )
            .await?;
        Ok(())
    }

    #[derive(Deserialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl ReadEntity for Contact {}

    impl Select for Contact {
        fn get_columns() -> &'static [&'static str] {
            &["contactid", "firstname", "lastname"]
        }
    }
    ```
    */
    pub async fn retrieve<E: ReadEntity>(&self, reference: &impl Reference) -> Result<E> {
        let reference = reference.get_reference();
        let columns = E::get_columns();
        let url_path = self.build_retrieve_url(reference.entity_name, reference.entity_id, columns);

        async fn handle_response<E: ReadEntity>(response: Response) -> Result<E> {
            if response.status().is_client_error() || response.status().is_server_error() {
                let error_message = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("no error details provided from server"));
                return Err(DataverseError::new(error_message));
            }
    
            let content = response.bytes().await.into_dataverse_result()?;
            serde_json::from_slice(content.as_ref()).into_dataverse_result()
        }

        self.request(
            Method::GET, 
            &url_path, 
            move |request| Ok(request), 
            handle_response
        ).await
    }

    /**
    Executes the query and retrieves the entities from dataverse

    This function uses the implementation of the `Select` trait to only retrieve
    those attributes relevant to the struct defined. It is an Anti-Pattern to
    retrieve all attributes when they are not needed, so this library does not
    give the option to do that

    Please note that if you don't specify a limit then the client will try to retrieve
    up to 5000 records. Further records can then be retrieved with the `retrieve_next_page()`
    function

    This may fail for any of these reasons
    - An authentication failure
    - A serde deserialization error
    - Any http client or server error

    # Examples
    ```rust
    use uuid::Uuid;
    use serde::Deserialize;
    use powerplatform_dataverse_service_client::{
        client::{Client, Page},
        entity::ReadEntity,
        reference::ReferenceStruct,
        result::{IntoDataverseResult, Result},
        select::Select,
        query::Query
    };

    async fn test() -> Result<()> {
        // this query retrieves the first 3 contacts
        let query = Query::new("contacts").limit(3);
        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        let contacts: Page<Contact> = client.retrieve_multiple(&query).await?;
        Ok(())
    }

    #[derive(Deserialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl ReadEntity for Contact {}

    impl Select for Contact {
        fn get_columns() -> &'static [&'static str] {
            &["contactid", "firstname", "lastname"]
        }
    }
    ```
    */
    pub async fn retrieve_multiple<E: ReadEntity>(&self, query: &Query) -> Result<Page<E>> {
        let columns = E::get_columns();
        let url_path = self.build_query_url(columns, query);

        async fn handle_response<E: ReadEntity>(response: Response) -> Result<Page<E>> {
            if response.status().is_client_error() || response.status().is_server_error() {
                let error_message = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("no error details provided from server"));
                return Err(DataverseError::new(error_message));
            }
    
            let content = response.bytes().await.into_dataverse_result()?;
            let result = serde_json::from_slice(content.as_ref()).into_dataverse_result()?;
    
            match result {
                RetrieveMultipleResult {entities, next_link} => {
                    Ok(Page::new(entities, next_link))
                }
            }
        }

        self.request(
            Method::GET, 
            &url_path, 
            move |request| Ok(request),
            handle_response
        ).await
    }

    /**
    Continues a previous query by fetching the next records after a `Page`

    You can check with `is_incomplete()` if there are further records available to a query 

    This may fail for any of these reasons
    - An authentication failure
    - A serde deserialization error
    - Any http client or server error
    - The query already finished with the last page

    # Examples
    ```rust
    use uuid::Uuid;
    use serde::Deserialize;
    use powerplatform_dataverse_service_client::{
        client::{Client, Page},
        entity::ReadEntity,
        reference::ReferenceStruct,
        result::{IntoDataverseResult, Result},
        select::Select,
        query::Query
    };

    async fn test() -> Result<()> {
        let query = Query::new("contacts");
        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        let contact_page1: Page<Contact> = client.retrieve_multiple(&query).await?;

        if contact_page1.is_incomplete() {
            let contact_page2 = client.retrieve_next_page(&contact_page1).await?;
        }

        Ok(())
    }

    #[derive(Deserialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl ReadEntity for Contact {}

    impl Select for Contact {
        fn get_columns() -> &'static [&'static str] {
            &["contactid", "firstname", "lastname"]
        }
    }
    ```
    */
    pub async fn retrieve_next_page<E: ReadEntity>(&self, previous_page: &Page<E>) -> Result<Page<E>> {
        if previous_page.next_link.is_none() {
            return Err(DataverseError::new(String::from("There is no next page to retrieve")))
        }
        
        async fn handle_response<E: ReadEntity>(response: Response) -> Result<Page<E>> {
            if response.status().is_client_error() || response.status().is_server_error() {
                let error_message = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("no error details provided from server"));
                return Err(DataverseError::new(error_message));
            }
    
            let content = response.bytes().await.into_dataverse_result()?;
            let result = serde_json::from_slice(content.as_ref()).into_dataverse_result()?;
    
            match result {
                RetrieveMultipleResult {entities, next_link} => {
                    Ok(Page::new( entities, next_link))
                }
            }
        }

        self.request(
            Method::GET, 
            previous_page.next_link.as_ref().unwrap(), 
            move |request| Ok(request),
            handle_response
        ).await
    }

    /**
    executes the batch against the dataverse environment

    This function will fail if:
    - the batch size exceeds 1000 calls
    - the batch execution time exceeds 2 minutes

    the second restriction is especially tricky to handle because the execution time
    depends on the complexity of the entity in dataverse.
    So it is possible to create 300 records of an entity with low complexity
    but only 50 records of an entity with high complexity in that timeframe.

    Based on experience a batch size of 50 should be safe for all entities though

    # Examples
    ```rust
    use uuid::Uuid;
    use serde::Serialize;
    use powerplatform_dataverse_service_client::{
        batch::Batch,
        client::Client,
        entity::WriteEntity,
        reference::{Reference, ReferenceStruct},
        result::{IntoDataverseResult, Result}
    };

    async fn test() -> Result<()> {
        let testy_contact = Contact {
            contactid: Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?,
            firstname: String::from("Testy"),
            lastname: String::from("McTestface"),
        };

        let marianne_contact = Contact {
            contactid: Uuid::parse_str("12345678-1234-1234-1234-123456789abc").into_dataverse_result()?,
            firstname: String::from("Marianne"),
            lastname: String::from("McTestface"),
        };

        // this batch creates both contacts in one call
        let mut batch = Batch::new("https://instance.crm.dynamics.com/");
        batch.create(&testy_contact)?;
        batch.create(&marianne_contact)?;
        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        client.execute(&batch).await?;
        Ok(())
    }

    #[derive(Serialize)]
    struct Contact {
        contactid: Uuid,
        firstname: String,
        lastname: String,
    }

    impl WriteEntity for Contact {}

    impl Reference for Contact {
        fn get_reference(&self) -> ReferenceStruct {
            ReferenceStruct::new(
                "contacts",
                self.contactid,
            )
        }
    }
    ```
    */
    pub async fn execute(&self, batch: &Batch) -> Result<()> {
        let url_path = self.build_simple_url("$batch");

        self.request(
            Method::POST, 
            &url_path, 
            move |request| {
                Ok(request
                    .header("Content-Type", format!("multipart/mixed; boundary=batch_{}", batch.get_batch_id()))
                    .body(batch.to_string())
                )
            }, 
            handle_empty_response
        ).await
    }

    /**
    Tries to merge two entities with and deactivates the subordinate after the process

    This method is only supported for the following entities:
    - account
    - contact
    - lead
    - incident

    # Examples
    ```rust
    use uuid::Uuid;
    use powerplatform_dataverse_service_client::client::Client;
    use powerplatform_dataverse_service_client::result::{IntoDataverseResult, Result};

    async fn test() -> Result<()> {
        let target_id = Uuid::parse_str("12345678-1234-1234-1234-123456789012").into_dataverse_result()?;
        let subordinate_id = Uuid::parse_str("12345687-1234-1234-1234-123456879012").into_dataverse_result()?;

        let client = Client::new_dummy(); // Please replace this with your preferred authentication method
        client.merge("account", target_id, subordinate_id).await
    }
    ```
    */
    pub async fn merge(&self, entity_name: impl Display, target: Uuid, subordinate: Uuid) -> Result<()> {
        let url_path = self.build_simple_url("Merge");

        self.request(
            Method::POST,
            &url_path, 
            move |request| {
                let entity_name = entity_name.to_string();
                let merge_request = MergeRequest::new(&entity_name, target, subordinate, false);

                Ok(request
                    .header("Content-Type", "application/json")
                    .body(serde_json::to_vec(&merge_request).into_dataverse_result()?)
                )
            }, 
            handle_empty_response,
        ).await
    }

    async fn request<E, Fut>(
        &self,
        method: Method,
        url: &str, 
        request_preparer: impl FnOnce(RequestBuilder) -> Result<RequestBuilder>,
        response_consumer: impl FnOnce(Response) -> Fut,
    ) -> Result<E> 
    where Fut: Future<Output = Result<E>>{
        let token = self.auth.get_valid_token().await?;

        let response = request_preparer(self.backend.request(method, url))?
            .bearer_auth(token)
            .header("OData-MaxVersion", "4.0")
            .header("OData-Version", "4.0")
            .header("Accept", "application/json")
            .send().await.into_dataverse_result()?;

        response_consumer(response).await
    }

    fn build_simple_url(&self, table_name: impl Display) -> String {
        format!("{}api/data/v{}/{}", self.url, VERSION, table_name)
    }

    fn build_targeted_url(&self, table_name: impl Display, target_id: Uuid) -> String {
        format!(
            "{}api/data/v{}/{}({})",
            self.url,
            VERSION,
            table_name,
            target_id.as_hyphenated()
        )
    }

    fn build_retrieve_url(&self, table_name: impl Display, target_id: Uuid, columns: &[&str]) -> String {
        let mut select = String::new();
        let mut comma_required = false;

        for column in columns {
            if comma_required {
                select.push(',');
            }

            select.push_str(column);
            comma_required = true;
        }

        format!(
            "{}api/data/v{}/{}({})?$select={}",
            self.url,
            VERSION,
            table_name,
            target_id.as_hyphenated(),
            select
        )
    }

    fn build_query_url(&self, columns: &[&str], query: &Query) -> String {
        let mut select = String::new();
        let mut comma_required = false;

        for column in columns {
            if comma_required {
                select.push(',');
            }

            select.push_str(column);
            comma_required = true;
        }

        format!(
            "{}api/data/v{}/{}&$select={}",
            self.url, VERSION, query, select
        )
    }
}

async fn handle_empty_response(response: Response) -> Result<()> {
    if response.status().is_client_error() || response.status().is_server_error() {
        let error_message = response.text().await.unwrap_or_else(|_| String::from("no error details provided from server"));
        return Err(DataverseError::new(error_message));
    }

    Ok(())
}

/**
A page of retrieved entites by the `retrieve_multiple()` and `retrieve_next_page()`
by a client instance 
*/
#[derive(Debug)]
pub struct Page<E> {
    pub entities: Vec<E>,
    next_link: Option<String>,
}

impl<E> Page<E> {
    fn new(entities: Vec<E>, next_link: Option<String>) -> Self {
        Self {
            entities,
            next_link,
        }
    }

    /// Indicates if there are more records available in the query after this page
    pub fn is_incomplete(&self) -> bool {
        self.next_link.is_some()
    }

    /// Transforms the page into its content as a `Vec`
    pub fn into_inner(self) -> Vec<E> {
        self.entities
    }
}

#[derive(Deserialize)]
struct RetrieveMultipleResult<E> {
    #[serde(rename = "value")]
    entities: Vec<E>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}