use kube::CustomResourceExt;
use sarena_kubernetes::crd;

fn main() {
    println!("---");
    print!(
        "{}",
        serde_yaml::to_string(&crd::address_pool::AddressPool::crd()).unwrap()
    );
}
