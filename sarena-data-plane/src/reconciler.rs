use std::{collections::HashMap, net::IpAddr, sync::Arc};

use ipnet::IpNet;

use crate::{Dataplane, Endpoint, EndpointId, Node, NodeId, Route, Service};

#[derive(Clone, Default)]
pub struct DesiredState {
    pub nodes: HashMap<NodeId, Node>,
    pub endpoints: HashMap<EndpointId, Endpoint>,
    pub routes: HashMap<IpNet, Route>,
    pub services: HashMap<IpAddr, Service>,
}

pub struct Reconciler<D: Dataplane> {
    dataplane: Arc<D>,
}

impl<D: Dataplane> Reconciler<D> {
    pub async fn reconcile(&self, desired: &DesiredState) -> Result<(), D::Err> {
        let current_routes = self.dataplane.dump_routes().await?;
        let current: HashMap<_, _> = current_routes
            .into_iter()
            .map(|r| (r.destination, r))
            .collect();

        for (dest, route) in &desired.routes {
            if current.get(dest) != Some(route) {
                self.dataplane.add_route(route.clone()).await?;
            }
        }
        for dest in current.keys() {
            if !desired.routes.contains_key(dest) {
                // self.dataplane.remove_route(*dest).await?;
            }
        }
        Ok(())
    }
}
