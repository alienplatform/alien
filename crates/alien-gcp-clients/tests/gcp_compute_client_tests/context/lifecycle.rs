use super::*;

impl AsyncTestContext for ComputeTestContext {
    async fn setup() -> ComputeTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: TEST_REGION.to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = ComputeClient::new(Client::new(), config);

        ComputeTestContext {
            client,
            project_id,
            region: TEST_REGION.to_string(),
            zone: TEST_ZONE.to_string(),
            created_networks: Mutex::new(HashSet::new()),
            created_subnetworks: Mutex::new(HashSet::new()),
            created_routers: Mutex::new(HashSet::new()),
            created_firewalls: Mutex::new(HashSet::new()),
            created_health_checks: Mutex::new(HashSet::new()),
            created_backend_services: Mutex::new(HashSet::new()),
            created_url_maps: Mutex::new(HashSet::new()),
            created_target_http_proxies: Mutex::new(HashSet::new()),
            created_global_addresses: Mutex::new(HashSet::new()),
            created_global_forwarding_rules: Mutex::new(HashSet::new()),
            created_negs: Mutex::new(HashSet::new()),
            created_instance_templates: Mutex::new(HashSet::new()),
            created_instance_group_managers: Mutex::new(HashSet::new()),
            created_disks: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Compute Engine test cleanup...");

        // Clean up in reverse dependency order

        // Delete disks (must be detached first)
        let disks_to_cleanup = {
            let disks = self.created_disks.lock().unwrap();
            disks.clone()
        };
        for (zone, disk_name) in disks_to_cleanup {
            self.cleanup_disk(&zone, &disk_name).await;
        }

        // Delete instance group managers (must be done before instance templates)
        let igms_to_cleanup = {
            let igms = self.created_instance_group_managers.lock().unwrap();
            igms.clone()
        };
        for (zone, igm_name) in igms_to_cleanup {
            self.cleanup_instance_group_manager(&zone, &igm_name).await;
        }

        // Delete instance templates
        let templates_to_cleanup = {
            let templates = self.created_instance_templates.lock().unwrap();
            templates.clone()
        };
        for template_name in templates_to_cleanup {
            self.cleanup_instance_template(&template_name).await;
        }

        // Delete forwarding rules (must be before target proxies and addresses)
        let fwds_to_cleanup = {
            let fwds = self.created_global_forwarding_rules.lock().unwrap();
            fwds.clone()
        };
        for fwd_name in fwds_to_cleanup {
            self.cleanup_global_forwarding_rule(&fwd_name).await;
        }

        // Delete target HTTP proxies (must be before URL maps)
        let proxies_to_cleanup = {
            let proxies = self.created_target_http_proxies.lock().unwrap();
            proxies.clone()
        };
        for proxy_name in proxies_to_cleanup {
            self.cleanup_target_http_proxy(&proxy_name).await;
        }

        // Delete URL maps (must be before backend services)
        let url_maps_to_cleanup = {
            let url_maps = self.created_url_maps.lock().unwrap();
            url_maps.clone()
        };
        for url_map_name in url_maps_to_cleanup {
            self.cleanup_url_map(&url_map_name).await;
        }

        // Delete backend services (must be before health checks and NEGs)
        let bs_to_cleanup = {
            let bs = self.created_backend_services.lock().unwrap();
            bs.clone()
        };
        for bs_name in bs_to_cleanup {
            self.cleanup_backend_service(&bs_name).await;
        }

        // Delete NEGs
        let negs_to_cleanup = {
            let negs = self.created_negs.lock().unwrap();
            negs.clone()
        };
        for (zone, neg_name) in negs_to_cleanup {
            self.cleanup_neg(&zone, &neg_name).await;
        }

        // Delete health checks
        let hc_to_cleanup = {
            let hc = self.created_health_checks.lock().unwrap();
            hc.clone()
        };
        for hc_name in hc_to_cleanup {
            self.cleanup_health_check(&hc_name).await;
        }

        // Delete global addresses
        let addrs_to_cleanup = {
            let addrs = self.created_global_addresses.lock().unwrap();
            addrs.clone()
        };
        for addr_name in addrs_to_cleanup {
            self.cleanup_global_address(&addr_name).await;
        }

        // Delete firewalls
        let firewalls_to_cleanup = {
            let firewalls = self.created_firewalls.lock().unwrap();
            firewalls.clone()
        };
        for firewall_name in firewalls_to_cleanup {
            self.cleanup_firewall(&firewall_name).await;
        }

        // Delete routers
        let routers_to_cleanup = {
            let routers = self.created_routers.lock().unwrap();
            routers.clone()
        };
        for (region, router_name) in routers_to_cleanup {
            self.cleanup_router(&region, &router_name).await;
        }

        // Delete subnetworks
        let subnetworks_to_cleanup = {
            let subnetworks = self.created_subnetworks.lock().unwrap();
            subnetworks.clone()
        };
        for (region, subnetwork_name) in subnetworks_to_cleanup {
            self.cleanup_subnetwork(&region, &subnetwork_name).await;
        }

        // Delete networks
        let networks_to_cleanup = {
            let networks = self.created_networks.lock().unwrap();
            networks.clone()
        };
        for network_name in networks_to_cleanup {
            self.cleanup_network(&network_name).await;
        }

        info!("✅ Compute Engine test cleanup completed");
    }
}
