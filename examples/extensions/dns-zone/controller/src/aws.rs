use acme_types::{AwsDnsZoneFields, DnsZone, DnsZoneOutputs};
use alien_extension_sdk::controller_prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[controller]
pub struct AwsDnsZoneController {
    #[serde(flatten)]
    fields: AwsDnsZoneFields, // the same fields the importer fills
    /// The caller reference this zone's CreateHostedZone sends. Route53 never reuses one, even
    /// after its zone is deleted, so each create picks a new one and saves it before the call.
    /// A zone the importer read from the setup template has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    caller_reference: Option<String>,
}

#[controller]
impl AwsDnsZoneController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        // Saved by this step, so a CreateZone that runs again after a lost reply sends the same
        // reference and finds the zone it made.
        self.caller_reference = Some(caller_reference(
            ctx.resource_prefix,
            &zone.id,
            SystemTime::now(),
        ));
        Ok(HandlerAction::Continue {
            state: CreateZone,
            suggested_delay: None,
        })
    }

    #[handler(state = CreateZone, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_zone(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        let reference = self.caller_reference.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "The zone has no caller reference to create it with".to_string(),
                resource_id: Some(zone.id.clone()),
            })
        })?;
        let route53 = aws_sdk_route53::Client::new(&ctx.aws_sdk_config().await?);
        let zone_id = create_hosted_zone(&route53, &zone, &reference).await?;
        // CloudFormation's Ref returns "Z123", so keep that form on both setup paths.
        self.fields.zone_id = Some(zone_id.trim_start_matches("/hostedzone/").to_string());
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // Check that the zone still exists.
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // Route53 can't rename a hosted zone, and the delete finds the zone's apex by its domain.
        let previous = ctx.previous_resource_config::<DnsZone>()?;
        let zone = ctx.desired_resource_config::<DnsZone>()?;
        if !same_domain(&previous.domain, &zone.domain) {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "A Route53 hosted zone can't be renamed from {} to {}; remove the zone from the stack and add it back with the new domain",
                    previous.domain, zone.domain
                ),
                resource_id: Some(zone.id.clone()),
            }));
        }
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // No id: the create never made a zone.
        if let Some(zone_id) = self.fields.zone_id.clone() {
            let zone = ctx.desired_resource_config::<DnsZone>()?;
            let route53 = aws_sdk_route53::Client::new(&ctx.aws_sdk_config().await?);
            delete_hosted_zone(&route53, &zone_id, &zone.domain, &zone.id).await?;
            self.fields.zone_id = None;
        }
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let zone_id = self.fields.zone_id.clone()?;
        Some(ResourceOutputs::new(DnsZoneOutputs { zone_id }))
    }
}

/// A caller reference for a zone whose create starts at `now`: unique per deployment and zone, and
/// per create, at most 128 characters.
fn caller_reference(resource_prefix: &str, zone_id: &str, now: SystemTime) -> String {
    let millis = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{resource_prefix}-{zone_id}-{millis}")
}

/// Creates the hosted zone for `zone` with `reference`, and returns its id. A reference that
/// already made a zone (the reply to an earlier call was lost) returns that zone. A reference
/// whose zone is gone fails: Route53 won't create another with it.
async fn create_hosted_zone(
    route53: &aws_sdk_route53::Client,
    zone: &DnsZone,
    reference: &str,
) -> Result<String> {
    let failed = |message: &str| ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: Some(zone.id.clone()),
    };
    let request = route53
        .create_hosted_zone()
        .name(&zone.domain)
        .caller_reference(reference);
    let error = match request.send().await {
        Ok(created) => {
            return created
                .hosted_zone()
                .map(|hosted_zone| hosted_zone.id().to_string())
                .ok_or_else(|| {
                    AlienError::new(failed("Route53 created the hosted zone and returned no id"))
                })
        }
        Err(error) => error,
    };
    if !error
        .as_service_error()
        .is_some_and(|error| error.is_hosted_zone_already_exists())
    {
        return Err(error)
            .into_alien_error()
            .context(failed("Failed to create the Route53 hosted zone"));
    }
    let zones = route53
        .list_hosted_zones_by_name()
        .dns_name(&zone.domain)
        .send()
        .await
        .into_alien_error()
        .context(failed(
            "Failed to list the Route53 hosted zones for the domain",
        ))?;
    zones
        .hosted_zones()
        .iter()
        .find(|hosted_zone| hosted_zone.caller_reference() == reference)
        .map(|hosted_zone| hosted_zone.id().to_string())
        .ok_or_else(|| {
            AlienError::new(failed(&format!(
                "Route53 has already used the caller reference {reference} for a hosted zone that no longer exists, and a retry sends the same reference. Remove the zone from the stack and add it back, so its create picks a new one"
            )))
        })
}

/// Deletes the hosted zone `zone_id` for `domain`, after the record sets Route53 lets it delete:
/// all but the SOA and NS records at the zone's apex, which go with the zone. A zone that is already gone is
/// deleted, so a step that runs again after a lost reply succeeds.
async fn delete_hosted_zone(
    route53: &aws_sdk_route53::Client,
    zone_id: &str,
    domain: &str,
    resource_id: &str,
) -> Result<()> {
    use aws_sdk_route53::types::{Change, ChangeAction, ChangeBatch, RrType};

    let failed = |message: &str| ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: Some(resource_id.to_string()),
    };
    let mut records = Vec::new();
    // Where the next page starts: a name, a type, and for weighted or other routed records, the
    // set's identifier.
    let mut next: Option<(String, RrType, Option<String>)> = None;
    loop {
        let mut request = route53.list_resource_record_sets().hosted_zone_id(zone_id);
        if let Some((name, record_type, identifier)) = next.take() {
            request = request
                .start_record_name(name)
                .start_record_type(record_type)
                .set_start_record_identifier(identifier);
        }
        let page = match request.send().await {
            Ok(page) => page,
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|error| error.is_no_such_hosted_zone()) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(error).into_alien_error().context(failed(
                    "Failed to list the Route53 hosted zone's record sets",
                ))
            }
        };
        records.extend(page.resource_record_sets().iter().cloned());
        match (
            page.is_truncated(),
            page.next_record_name(),
            page.next_record_type(),
        ) {
            (true, Some(name), Some(record_type)) => {
                next = Some((
                    name.to_string(),
                    record_type.clone(),
                    page.next_record_identifier().map(str::to_string),
                ))
            }
            _ => break,
        }
    }
    let changes = records
        .into_iter()
        .filter(|record| {
            let at_apex = same_domain(record.name(), domain);
            !(at_apex && matches!(record.r#type(), RrType::Soa | RrType::Ns))
        })
        .map(|record| {
            Change::builder()
                .action(ChangeAction::Delete)
                .resource_record_set(record)
                .build()
                .into_alien_error()
                .context(failed("Failed to build the record set's delete"))
        })
        .collect::<Result<Vec<_>>>()?;
    for chunk in change_batches(changes) {
        let batch = ChangeBatch::builder()
            .set_changes(Some(chunk))
            .build()
            .into_alien_error()
            .context(failed("Failed to build the record sets' delete"))?;
        let deleted = route53
            .change_resource_record_sets()
            .hosted_zone_id(zone_id)
            .change_batch(batch)
            .send()
            .await;
        match deleted {
            Ok(_) => {}
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|error| error.is_no_such_hosted_zone()) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(error).into_alien_error().context(failed(
                    "Failed to delete the Route53 hosted zone's record sets",
                ))
            }
        }
    }
    match route53.delete_hosted_zone().id(zone_id).send().await {
        Ok(_) => Ok(()),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(|error| error.is_no_such_hosted_zone()) =>
        {
            Ok(())
        }
        Err(error) => Err(error)
            .into_alien_error()
            .context(failed("Failed to delete the Route53 hosted zone")),
    }
}

/// Most `ResourceRecord` elements one ChangeResourceRecordSets request may hold, an alias record
/// counting as one.
const MAX_RECORDS_PER_REQUEST: usize = 1_000;
/// Most characters one request's `Value` elements may hold in all.
const MAX_VALUE_CHARACTERS_PER_REQUEST: usize = 32_000;

/// `changes` in order, split into requests within Route53's quotas on records and on value
/// characters per request. A record set over a quota on its own can't be split, so it goes
/// alone, and Route53's refusal names it.
fn change_batches(
    changes: Vec<aws_sdk_route53::types::Change>,
) -> Vec<Vec<aws_sdk_route53::types::Change>> {
    let mut batches: Vec<Vec<_>> = Vec::new();
    let (mut records, mut characters) = (0, 0);
    for change in changes {
        // Every change here was built with its record set.
        let (change_records, change_characters) =
            change.resource_record_set().map_or((0, 0), |set| {
                let values = set.resource_records();
                let records = if values.is_empty() && set.alias_target().is_some() {
                    1
                } else {
                    values.len()
                };
                let characters = values
                    .iter()
                    .map(|value| value.value().chars().count())
                    .sum::<usize>();
                (records, characters)
            });
        let fits = records + change_records <= MAX_RECORDS_PER_REQUEST
            && characters + change_characters <= MAX_VALUE_CHARACTERS_PER_REQUEST;
        match batches.last_mut() {
            Some(batch) if fits => batch.push(change),
            _ => {
                batches.push(vec![change]);
                (records, characters) = (0, 0);
            }
        }
        records += change_records;
        characters += change_characters;
    }
    batches
}

/// Whether two DNS names are the same name: DNS compares names case-insensitively, and Route53
/// writes them with a trailing dot.
fn same_domain(a: &str, b: &str) -> bool {
    a.trim_end_matches('.')
        .eq_ignore_ascii_case(b.trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const XMLNS: &str = "https://route53.amazonaws.com/doc/2013-04-01/";

    /// A Route53 that answers each request with `answer(request)`, a status and an XML body, and
    /// records each request it gets.
    async fn stand_in_route53(
        answer: impl Fn(&str) -> (&'static str, String) + Send + Sync + 'static,
    ) -> (aws_sdk_route53::Client, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let answer = Arc::new(answer);
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (recorded, answer) = (recorded.clone(), answer.clone());
                tokio::spawn(async move {
                    let request = read_request(&mut socket).await;
                    let (status, body) = answer(&request);
                    recorded.lock().unwrap().push(request);
                    let reply = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/xml\r\nx-amzn-requestid: r1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let config = aws_sdk_route53::Config::builder()
            .behavior_version(aws_sdk_route53::config::BehaviorVersion::latest())
            .region(aws_sdk_route53::config::Region::new("us-east-1"))
            .credentials_provider(aws_sdk_route53::config::Credentials::new(
                "AKIDTEST", "secret", None, None, "test",
            ))
            .endpoint_url(url)
            .build();
        (aws_sdk_route53::Client::from_conf(config), requests)
    }

    /// The request line, headers and body, read up to the body's Content-Length.
    async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = socket.read(&mut buf).await.unwrap_or(0);
            bytes.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&bytes).to_string();
            let Some(end) = text.find("\r\n\r\n") else {
                if n == 0 {
                    return text;
                }
                continue;
            };
            let length = text[..end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if n == 0 || bytes.len() >= end + 4 + length {
                return text;
            }
        }
    }

    fn error(status: &'static str, code: &str) -> (&'static str, String) {
        (
            status,
            format!("<ErrorResponse xmlns=\"{XMLNS}\"><Error><Type>Sender</Type><Code>{code}</Code><Message>{code}</Message></Error><RequestId>r1</RequestId></ErrorResponse>"),
        )
    }

    fn change_info(response: &str) -> (&'static str, String) {
        (
            "200 OK",
            format!("<{response} xmlns=\"{XMLNS}\"><ChangeInfo><Id>/change/C1</Id><Status>PENDING</Status><SubmittedAt>2026-11-10T00:00:00Z</SubmittedAt></ChangeInfo></{response}>"),
        )
    }

    /// Answers CreateHostedZone with HostedZoneAlreadyExists, and lists one zone for the domain,
    /// made with `existing_reference`.
    fn a_zone_made_by(existing_reference: &'static str) -> impl Fn(&str) -> (&'static str, String) {
        move |request: &str| {
            if request.starts_with("POST /2013-04-01/hostedzone ") {
                error("409 Conflict", "HostedZoneAlreadyExists")
            } else {
                ("200 OK", format!("<ListHostedZonesByNameResponse xmlns=\"{XMLNS}\"><HostedZones><HostedZone><Id>/hostedzone/Z1</Id><Name>internal.acme.dev.</Name><CallerReference>{existing_reference}</CallerReference><ResourceRecordSetCount>2</ResourceRecordSetCount></HostedZone></HostedZones><DNSName>internal.acme.dev</DNSName><IsTruncated>false</IsTruncated><MaxItems>100</MaxItems></ListHostedZonesByNameResponse>"))
            }
        }
    }

    fn record(name: &str, record_type: &str, value: &str) -> String {
        records(name, record_type, &[value])
    }

    fn records(name: &str, record_type: &str, values: &[&str]) -> String {
        let values: String = values
            .iter()
            .map(|value| format!("<ResourceRecord><Value>{value}</Value></ResourceRecord>"))
            .collect();
        format!("<ResourceRecordSet><Name>{name}</Name><Type>{record_type}</Type><TTL>300</TTL><ResourceRecords>{values}</ResourceRecords></ResourceRecordSet>")
    }

    /// One page of ListResourceRecordSets, and where the next starts when there is one.
    fn record_page(sets: &str, next: Option<(&str, &str, &str)>) -> (&'static str, String) {
        let next = next
            .map(|(name, record_type, identifier)| format!("<NextRecordName>{name}</NextRecordName><NextRecordType>{record_type}</NextRecordType><NextRecordIdentifier>{identifier}</NextRecordIdentifier>"))
            .unwrap_or_default();
        let truncated = !next.is_empty();
        ("200 OK", format!("<ListResourceRecordSetsResponse xmlns=\"{XMLNS}\"><ResourceRecordSets>{sets}</ResourceRecordSets><IsTruncated>{truncated}</IsTruncated>{next}<MaxItems>300</MaxItems></ListResourceRecordSetsResponse>"))
    }

    /// A zone Z1 whose listing is `sets`, which accepts every change and the zone's delete.
    fn a_zone_listing(sets: String) -> impl Fn(&str) -> (&'static str, String) {
        move |request: &str| {
            if request.starts_with("GET /2013-04-01/hostedzone/Z1/rrset") {
                record_page(&sets, None)
            } else if request.starts_with("POST /2013-04-01/hostedzone/Z1/rrset") {
                change_info("ChangeResourceRecordSetsResponse")
            } else if request.starts_with("DELETE /2013-04-01/hostedzone/Z1 ") {
                change_info("DeleteHostedZoneResponse")
            } else {
                error("400 Bad Request", "InvalidInput")
            }
        }
    }

    /// The change batches the stand-in got, in order.
    fn batches(requests: &[String]) -> Vec<&String> {
        requests
            .iter()
            .filter(|request| request.starts_with("POST /2013-04-01/hostedzone/Z1/rrset"))
            .collect()
    }

    /// The zone Z1 with its apex SOA and NS, and an A record a workload wrote.
    fn a_zone_with_a_record(request: &str) -> (&'static str, String) {
        if request.starts_with("GET /2013-04-01/hostedzone/Z1/rrset") {
            let records = [
                record(
                    "internal.acme.dev.",
                    "SOA",
                    "ns-1.awsdns-00.com. hostmaster.amazon.com. 1 7200 900 1209600 86400",
                ),
                record("internal.acme.dev.", "NS", "ns-1.awsdns-00.com."),
                record("www.internal.acme.dev.", "A", "10.0.0.1"),
            ]
            .concat();
            ("200 OK", format!("<ListResourceRecordSetsResponse xmlns=\"{XMLNS}\"><ResourceRecordSets>{records}</ResourceRecordSets><IsTruncated>false</IsTruncated><MaxItems>300</MaxItems></ListResourceRecordSetsResponse>"))
        } else if request.starts_with("POST /2013-04-01/hostedzone/Z1/rrset") {
            change_info("ChangeResourceRecordSetsResponse")
        } else if request.starts_with("DELETE /2013-04-01/hostedzone/Z1 ") {
            change_info("DeleteHostedZoneResponse")
        } else {
            error("400 Bad Request", "InvalidInput")
        }
    }

    fn zone() -> DnsZone {
        DnsZone::new("zone".to_string())
            .domain("internal.acme.dev".to_string())
            .build()
    }

    // A CreateZone that runs again after a lost reply: its reference already made a zone.
    #[tokio::test]
    async fn a_create_sent_again_adopts_the_zone_its_reference_made() {
        let (route53, _) = stand_in_route53(a_zone_made_by("q6aa-zone-1")).await;
        let id = create_hosted_zone(&route53, &zone(), "q6aa-zone-1")
            .await
            .unwrap();
        assert_eq!(id, "/hostedzone/Z1");
    }

    // The zone that reference made was deleted, and Route53 won't make another with it: the
    // create fails rather than reaching Ready with no zone.
    #[tokio::test]
    async fn a_create_whose_reference_made_a_deleted_zone_fails() {
        let (route53, _) = stand_in_route53(a_zone_made_by("q6aa-zone-1")).await;
        let error = create_hosted_zone(&route53, &zone(), "q6aa-zone-2")
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("caller reference q6aa-zone-2"),
            "{error}"
        );
    }

    // A zone deleted and created again gets a reference Route53 hasn't seen.
    #[test]
    fn each_create_picks_its_own_reference() {
        let first = caller_reference("q6aa", "zone", UNIX_EPOCH + Duration::from_secs(1));
        let second = caller_reference("q6aa", "zone", UNIX_EPOCH + Duration::from_secs(2));
        assert_ne!(first, second);
        let longest = caller_reference(&"a".repeat(40), &"z".repeat(22), SystemTime::now());
        assert!(longest.len() <= 128, "{longest}");
    }

    // Route53 refuses to delete a zone with records of its own, so the A record goes first, in
    // one change batch, and the apex SOA and NS stay for the zone's delete.
    #[tokio::test]
    async fn delete_removes_the_records_then_the_zone() {
        let (route53, requests) = stand_in_route53(a_zone_with_a_record).await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        let requests = requests.lock().unwrap().clone();
        let lines: Vec<&str> = requests
            .iter()
            .map(|request| request.lines().next().unwrap_or(""))
            .collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].starts_with("GET /2013-04-01/hostedzone/Z1/rrset"),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with("POST /2013-04-01/hostedzone/Z1/rrset"),
            "{lines:?}"
        );
        assert!(
            lines[2].starts_with("DELETE /2013-04-01/hostedzone/Z1 "),
            "{lines:?}"
        );
        let batch = &requests[1];
        assert_eq!(
            batch.matches("<Action>DELETE</Action>").count(),
            1,
            "{batch}"
        );
        assert!(
            batch.contains("<Name>www.internal.acme.dev.</Name>"),
            "{batch}"
        );
        assert!(
            !batch.contains("<Type>SOA</Type>") && !batch.contains("<Type>NS</Type>"),
            "{batch}"
        );
    }

    // The stack's domain in other letter cases is the same zone: its apex records stay.
    #[tokio::test]
    async fn delete_keeps_the_apex_whatever_the_domain_s_case() {
        for domain in ["Internal.Acme.DEV", "INTERNAL.ACME.DEV."] {
            let (route53, requests) = stand_in_route53(a_zone_with_a_record).await;
            delete_hosted_zone(&route53, "Z1", domain, "zone")
                .await
                .unwrap();
            let requests = requests.lock().unwrap().clone();
            let batch = &requests[1];
            assert_eq!(
                batch.matches("<Action>DELETE</Action>").count(),
                1,
                "{domain}: {batch}"
            );
            assert!(
                !batch.contains("<Type>SOA</Type>") && !batch.contains("<Type>NS</Type>"),
                "{domain}: {batch}"
            );
        }
    }

    #[test]
    fn a_domain_is_the_same_in_any_case_and_with_a_trailing_dot() {
        assert!(same_domain("internal.acme.dev.", "Internal.Acme.DEV"));
        assert!(!same_domain("internal.acme.dev", "other.acme.dev"));
    }

    // A delete that runs again after the zone went: nothing left to delete, and no error.
    #[tokio::test]
    async fn delete_of_a_zone_that_is_gone_succeeds() {
        let (route53, requests) =
            stand_in_route53(|_: &str| error("404 Not Found", "NoSuchHostedZone")).await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    // 400 record sets of 3 values each are 1,200 ResourceRecord elements: more than one request
    // may hold, though they are only 400 changes.
    #[tokio::test]
    async fn delete_splits_its_changes_at_a_thousand_records() {
        let sets: String = (0..400)
            .map(|n| {
                records(
                    &format!("h{n}.internal.acme.dev."),
                    "A",
                    &["10.0.0.1", "10.0.0.2", "10.0.0.3"],
                )
            })
            .collect();
        let (route53, requests) = stand_in_route53(a_zone_listing(sets)).await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        let requests = requests.lock().unwrap().clone();
        let batches = batches(&requests);
        assert!(batches.len() >= 2, "{} batches", batches.len());
        for batch in &batches {
            assert!(batch.matches("<ResourceRecord>").count() <= 1_000);
        }
        let deleted: usize = batches
            .iter()
            .map(|batch| batch.matches("<Action>DELETE</Action>").count())
            .sum();
        assert_eq!(deleted, 400);
    }

    // 20 TXT sets of 2,000 characters are 40,000 characters of values: more than one request may
    // hold, though they are only 20 records.
    #[tokio::test]
    async fn delete_splits_its_changes_at_32000_value_characters() {
        let value = format!("\"{}\"", "a".repeat(1_998));
        let sets: String = (0..20)
            .map(|n| {
                records(
                    &format!("t{n}.internal.acme.dev."),
                    "TXT",
                    &[value.as_str()],
                )
            })
            .collect();
        let (route53, requests) = stand_in_route53(a_zone_listing(sets)).await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        let requests = requests.lock().unwrap().clone();
        let batches = batches(&requests);
        // 16 values of 2,000 characters fill a request to the 32,000 cap; the other 4 go next.
        let sets: Vec<usize> = batches
            .iter()
            .map(|batch| batch.matches("<Action>DELETE</Action>").count())
            .collect();
        assert_eq!(sets, vec![16, 4]);
    }

    // A listing over two pages, the second asked for by the first's next name, type and set
    // identifier. The apex NS stays though the listing has no SOA to say where the apex is.
    #[tokio::test]
    async fn delete_reads_every_page_and_keeps_the_apex_ns() {
        let (route53, requests) = stand_in_route53(|request: &str| {
            if request.starts_with("GET /2013-04-01/hostedzone/Z1/rrset?") {
                let second = request.contains("name=b.internal.acme.dev.")
                    && request.contains("type=A")
                    && request.contains("identifier=east");
                assert!(second, "{request}");
                record_page(&record("b.internal.acme.dev.", "A", "10.0.0.2"), None)
            } else if request.starts_with("GET /2013-04-01/hostedzone/Z1/rrset") {
                let sets = [
                    record("internal.acme.dev.", "NS", "ns-1.awsdns-00.com."),
                    record("a.internal.acme.dev.", "A", "10.0.0.1"),
                ]
                .concat();
                record_page(&sets, Some(("b.internal.acme.dev.", "A", "east")))
            } else if request.starts_with("POST /2013-04-01/hostedzone/Z1/rrset") {
                change_info("ChangeResourceRecordSetsResponse")
            } else if request.starts_with("DELETE /2013-04-01/hostedzone/Z1 ") {
                change_info("DeleteHostedZoneResponse")
            } else {
                error("400 Bad Request", "InvalidInput")
            }
        })
        .await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        let requests = requests.lock().unwrap().clone();
        let batches = batches(&requests);
        assert_eq!(batches.len(), 1);
        assert!(
            batches[0].contains("<Name>a.internal.acme.dev.</Name>"),
            "{}",
            batches[0]
        );
        assert!(
            batches[0].contains("<Name>b.internal.acme.dev.</Name>"),
            "{}",
            batches[0]
        );
        assert!(!batches[0].contains("<Type>NS</Type>"), "{}", batches[0]);
    }

    // The zone went between the listing and the records' delete: the delete is done, and it
    // sends no zone delete.
    #[tokio::test]
    async fn delete_whose_records_delete_finds_no_zone_succeeds() {
        let (route53, requests) = stand_in_route53(|request: &str| {
            if request.starts_with("POST ") {
                error("404 Not Found", "NoSuchHostedZone")
            } else {
                a_zone_with_a_record(request)
            }
        })
        .await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        let requests = requests.lock().unwrap().clone();
        assert!(
            !requests
                .iter()
                .any(|request| request.starts_with("DELETE ")),
            "{requests:?}"
        );
    }

    // The zone went between the records' delete and its own: the delete is done.
    #[tokio::test]
    async fn delete_whose_zone_delete_finds_no_zone_succeeds() {
        let (route53, requests) = stand_in_route53(|request: &str| {
            if request.starts_with("DELETE ") {
                error("404 Not Found", "NoSuchHostedZone")
            } else {
                a_zone_with_a_record(request)
            }
        })
        .await;
        delete_hosted_zone(&route53, "Z1", "internal.acme.dev", "zone")
            .await
            .unwrap();
        assert_eq!(requests.lock().unwrap().len(), 3);
    }
}
