// SPDX-License-Identifier: GPL-2.0

//! Virtual network driver for virtio.
//!
//! Based on the C driver written by Rusty Russell <rusty@rustcorp.com.au>.

use kernel::{
    amba, bit, bits_iter, define_amba_id_table, device, gpio,
    io_mem::IoMem,
    irq::{self, ExtraResult, IrqData, LockedIrqData},
    net, pages, power,
    prelude::*,
    scatterlist,
    sync::{RawSpinLock, Ref, RefBorrow},
    virtio, workqueue
};
use core::mem::size_of;

const guest_offloads : [u64;5] = {
	bindings::VIRTIO_NET_F_GUEST_TSO4;
	bindings::VIRTIO_NET_F_GUEST_TSO6;
	bindings::VIRTIO_NET_F_GUEST_ECN;
	bindings::VIRTIO_NET_F_GUEST_UFO;
	bindings::VIRTIO_NET_F_GUEST_CSUM
};

struct VirtioNetData {
    dev: ARef<device::Device>,
    vdev: virtio::Device,
    ndev: net::Device
}

struct VirtioNetResources {
    cvq: virtio::VirtQueue,
    sq: SendQueue,
    rq: ReceiveQueue,

    status: u32,

    // 	/* Max # of queue pairs supported by the device */
    max_queue_pairs: u16,

    // 	/* # of queue pairs currently used by the driver */
    curr_queue_pairs: u16,

    // 	/* # of XDP queue pairs currently used by the driver */
    xdp_queue_pairs: u16,

    // 	/* xdp_queue_pairs may be 0, when xdp is already loaded. So add this. */
    xdp_enabled: bool,

    // 	/* I like... big packets and I cannot lie! */
    big_packets: bool,

    // 	/* Host will merge rx buffers for big packets (shake it! shake it!) */
    mergeable_rx_bufs: bool,

    // 	/* Host supports rss and/or hash report */
    has_rss: bool,
    has_rss_hash_report: bool,
    rss_key_size: u8,
    rss_indir_table_size: u16,
    rss_hash_types_supported: u32,
    rss_hash_types_saved: u32,

    // 	/* Has control virtqueue */
    has_cvq: bool,

    // 	/* Host can handle any s/g split between our header and packet data */
    any_header_sg: bool,

    // 	/* Packet virtio header size */
    hdr_len: u8,

    // 	/* Work struct for refilling if we run low on memory. */
    refill: workqueue::DelayedWork,

    // 	/* Work struct for config space updates */
    config_work: workqueue::Work,

    // 	/* Does the affinity hint is set for virtqueues? */
    affinity_hint_set: bool,

    // 	/* CPU hotplug instances for online & dead */
    // node: struct hlist_node,
    // 	struct hlist_node node;
    // node_dead: struct hlist_node,
    // 	struct hlist_node node_dead;

    ctrl: control_buf,
    // 	struct control_buf *ctrl;

    // 	/* Ethtool settings */
    duplex: u8,
    speed: u32,

    //?
    guest_offloads: u32,
    //?
    guest_offloads_capable: u32,

    // 	/* failover when STANDBY feature enabled */
    // failover: struct failover,
    // 	struct failover *failover;
    // };
}

impl VirtioNetResources {

    fn init_vqs(&mut self, dev: ARef<device::Device>) -> Result<usize>{
        self.virtnet_alloc_queues()?;
    
        self.virtnet_find_vqs()?;
    
    // 	get_online_cpus();
    self.virtnet_set_affinity(dev)?;
    // 	put_online_cpus();
        Ok(0)
    }

    fn virtnet_set_affinity(&mut self, dev: ARef<device::Device>) -> Result<usize> {
        
    }

}

struct SendQueue {
    // 	/* Virtqueue associated with this send_queue */
    vq: virtio::VirtQueue,

    // 	/* TX: fragments + linear part + virtio header */
    sg: [scatterlist::Scatterlist; bindings::MAX_SKB_FRAGS as usize + 2],

    // 	/* Name of the send queue: output.$index */
    name: &'static CStr,

    stats: VirtnetSqStats,

    napi: net::napi_struct,
}

/* Internal representation of a receive virtqueue */
struct ReceiveQueue {
    // 	/* Virtqueue associated with this receive_queue */
    vq: virtio::VirtQueue,

    napi: net::napi_struct,

    // 	struct bpf_prog __rcu *xdp_prog;

    stats: virtnet_rq_stats,
    
    // 	/* Chain pages by the private ptr. */
    pages: pages::Pages<0>,

    // 	/* Average packet length for mergeable receive buffers. */
    // 	struct ewma_pkt_len mrg_avg_pkt_len;

    // 	/* Page frag for packet buffer allocation. */
    alloc_frag: pages::PageFrag,

    // 	/* RX: fragments + linear part + virtio header */
    sg: [scatterlist::Scatterlist; bindings::MAX_SKB_FRAGS as usize + 2],

    // 	/* Min single buffer size for mergeable buffers case. */
    min_buf_len: u32,

    // 	/* Name of this receive queue: input.$index */
    name: &'static CStr,

    // 	struct xdp_rxq_info xdp_rxq;
    // };
}

struct VirtnetSqStats {
    // syncp: struct u64_stats_sync,
    syncp: bindings::u64_stats_sync,
    packets: u64,
    bytes: u64,
    xdp_tx: u64,
    xdp_tx_drops: u64,
    kicks: u64,
    tx_timeouts: u64,
}

type DeviceData = device::Data<(), VirtioNetResources, VirtioNetData>;

module_virtio_driver! {
    type: VirtioNetDriver,
    name: b"virtio_net_rust",
    author: b"Rust for Linux Contributors",
    description: b"Virtio Net driver",
    license: b"GPL v2",
}

impl net::EthtoolOps for VirtioNetDriver{

}

impl net::NetdevOps for VirtioNetDriver{
    
}

struct VirtioNetDriver;
impl virtio::Driver for VirtioNetDriver {
    type Data = Ref<DeviceData>;

    define_virtio_id_table! {(), [
        ({ device: 0x00000001, vendor: 0xffffffff }, None),
    ]}

    fn probe(vdev: &mut virtio::Device) -> Result<Self::Data> {
        pr_info!("probing discovered virtio net with id {}\n", dev.id());
        let max_queue_pairs = 1;
        
        let mut reg = net::Registration::try_new(vdev)?;
        // let mut dev = net::Device::alloc_etherdev_mqs(
            // size_of::<u64>(), max_queue_pairs)?;
        
        let priv_flags = dev.priv_flags();

        dev.set_priv_flags( priv_flags | bindings::IFF_UNICAST_FLT as u32 | bindings::IFF_LIVE_ADDR_CHANGE as u32 | bindings::IFF_TX_SKB_NO_LINEAR as u32);
        
        let ethtool_ops = unsafe { net::NetDeviceTables::<Self>::ethtool_ops_build() };
        dev.set_ethtool_ops(ethtool_ops);

        dev.set_features(bindings::NETIF_F_HIGHDMA);

        let netdev_ops = unsafe { net::NetDeviceTables::<Self>::net_device_ops_build() };
        dev.set_netdev_ops(netdev_ops);

        dev.set_vlan_features(dev.features());

        dev.set_min_mtu(min_mtu) = bindings::MIN_MTU;
        dev.set_max_mtu(max_mtu) = bindings::MAX_MTU;

        let vinr = VirtioNetResources::new();
        vinr.set_max_queue_pairs(1);

        vinr.init_vqs(dev);

        dev.netif_set_real_num_tx_queues(vinr.curr_queue_pairs);
        dev.netif_set_real_num_tx_queues(vinr.curr_queue_pairs);

        vinr.set_speed(bindings::SPEED_UNKNOWN);
        vinr.set_duplex(bindings::DUPLEX_UNKNOWN);

	    dev.register_netdev()?;

	    vdev.virtio_device_ready()?;

        if vdev.virtio_has_feature(VIRTIO_NET_F_STATUS) {
            vinr.config_work.schedule_work();
            // todo
        } else {
            // todo
        }

        for i in guest_offloads {
            if vdev.virtio_has_feature(i){
                // set_bit
            }
        }
        vinr.guest_offloads_capable = vinr.get_guest_offloads;

        pr_debug!("virtnet: registered device %s with %d RX and TX vq's\n",
        dev.name, max_queue_pairs);

        let data = kernel::new_device_data!(
            (),
            vinr,
            VirtioNetData{
                dev: device::Device::from_dev(dev),
                vdev: vdev,
                ndev: dev
            },
            "Virtio::Registrations"
        )?;

        reg.register(data);

        Ok(data.into())
    }
}


// struct VirtioNetDevice;

// #[vtable]
// impl file::Operations for VirtioNetDevice {
//     fn open(_open_data: &(), _file: &File) -> Result {
//         Ok(())
//     }

//     fn read(_: (), _: &File, data: &mut impl IoBufferWriter, offset: u64) -> Result<usize> {
//         // Succeed if the caller doesn't provide a buffer or if not at the start.
//         if data.is_empty() || offset != 0 {
//             return Ok(0);
//         }

//         data.write(&0_u32)?;
//         Ok(4)
//     }
// }

// type DeviceData = device::Data<miscdev::Registration<RngDevice>, (), ()>;
// type DeviceData = device::Data<miscdev::Registration<RngDevice>, (), ()>;
