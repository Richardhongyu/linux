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

struct VirtnetInfo {
    vdev: ARef<virtio::Device>,
    cvq: virtio::VirtQueue,
    dev: net::Device,
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

    // ctrl: struct control_buf,
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

impl ReceiveQueue {
    fn new() -> Self {
        Self {

        }
    }
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

module_virtio_driver! {
    type: VirtioNetDriver,
    name: b"virtio_net_rust",
    author: b"Rust for Linux Contributors",
    description: b"Virtio Net driver",
    license: b"GPL v2",
}

struct VirtioNetDevice;

#[vtable]
impl file::Operations for VirtioNetDevice {
    fn open(_open_data: &(), _file: &File) -> Result {
        Ok(())
    }

    fn read(_: (), _: &File, data: &mut impl IoBufferWriter, offset: u64) -> Result<usize> {
        // Succeed if the caller doesn't provide a buffer or if not at the start.
        if data.is_empty() || offset != 0 {
            return Ok(0);
        }

        data.write(&0_u32)?;
        Ok(4)
    }
}

// type DeviceData = device::Data<miscdev::Registration<RngDevice>, (), ()>;
// type DeviceData = device::Data<miscdev::Registration<RngDevice>, (), ()>;

struct VirtioNetDriver;
impl virtio::Driver for VirtioNetDriver {
    type Data = Ref<DeviceData>;

    define_virtio_id_table! {(), [
        ({ device: 0x00000001, vendor: 0xffffffff }, None),
    ]}

    fn probe(dev: &mut virtio::Device) -> Result<Self::Data> {
        pr_info!("probing discovered virtio net with id {}\n", dev.id());
        let max_queue_pairs = 1;
        // let mut vi = VirtnetInfo{};
        
        let mut dev = unsafe{net::Device::alloc_etherdev_mqs(
            size_of::<VirtnetInfo>(), max_queue_pairs)?};
        dev.set_priv_flags(bindings::IFF_UNICAST_FLT as u32 | bindings::IFF_LIVE_ADDR_CHANGE as u32 | 
            bindings::IFF_TX_SKB_NO_LINEAR as u32);

        // vi.dev = dev;

        let data = find_vps();
        
        // let data = kernel::new_device_data!(
        //     miscdev::Registration::new(),
        //     (),
        //     (),
        //     "Virtio::Registrations"
        // )?;

        // data =
        // data.registrations()
        //     .ok_or(ENXIO)?
        //     .as_pinned_mut()
        //     .register(fmt!("rust_virtio_net"), ())?;
        Ok(data.into())

        //     int i, err = -ENOMEM;
        //     struct net_device *dev;
        //     struct virtnet_info *vi;
        //     u16 max_queue_pairs;
        //     int mtu;

        //     /* Find if host supports multiqueue/rss virtio_net device */
        //     max_queue_pairs = 1;
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_MQ) || virtio_has_feature(vdev, VIRTIO_NET_F_RSS))
        //         max_queue_pairs =
        //             virtio_cread16(vdev, offsetof(struct virtio_net_config, max_virtqueue_pairs));

        //     /* We need at least 2 queue's */
        //     if (max_queue_pairs < VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MIN ||
        //         max_queue_pairs > VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MAX ||
        //         !virtio_has_feature(vdev, VIRTIO_NET_F_CTRL_VQ))
        //         max_queue_pairs = 1;

        //     /* Allocate ourselves a network device with room for our info */
        //     dev = alloc_etherdev_mq(sizeof(struct virtnet_info), max_queue_pairs);
        //     if (!dev)
        //         return -ENOMEM;

        //     /* Set up network device as normal. */
        //     dev->priv_flags |= IFF_UNICAST_FLT | IFF_LIVE_ADDR_CHANGE |
        //             IFF_TX_SKB_NO_LINEAR;
        //     dev->netdev_ops = &virtnet_netdev;
        //     dev->features = NETIF_F_HIGHDMA;

        //     dev->ethtool_ops = &virtnet_ethtool_ops;
        //     SET_NETDEV_DEV(dev, &vdev->dev);

        //     /* Do we support "hardware" checksums? */
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_CSUM)) {
        //         /* This opens up the world of extra features. */
        //         dev->hw_features |= NETIF_F_HW_CSUM | NETIF_F_SG;
        //         if (csum)
        //             dev->features |= NETIF_F_HW_CSUM | NETIF_F_SG;

        //         if (virtio_has_feature(vdev, VIRTIO_NET_F_GSO)) {
        //             dev->hw_features |= NETIF_F_TSO
        //                 | NETIF_F_TSO_ECN | NETIF_F_TSO6;
        //         }
        //         /* Individual feature bits: what can host handle? */
        //         if (virtio_has_feature(vdev, VIRTIO_NET_F_HOST_TSO4))
        //             dev->hw_features |= NETIF_F_TSO;
        //         if (virtio_has_feature(vdev, VIRTIO_NET_F_HOST_TSO6))
        //             dev->hw_features |= NETIF_F_TSO6;
        //         if (virtio_has_feature(vdev, VIRTIO_NET_F_HOST_ECN))
        //             dev->hw_features |= NETIF_F_TSO_ECN;

        //         dev->features |= NETIF_F_GSO_ROBUST;

        //         if (gso)
        //             dev->features |= dev->hw_features & NETIF_F_ALL_TSO;
        //         /* (!csum && gso) case will be fixed by register_netdev() */
        //     }
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_CSUM))
        //         dev->features |= NETIF_F_RXCSUM;
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_TSO4) ||
        //         virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_TSO6))
        //         dev->features |= NETIF_F_GRO_HW;
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_CTRL_GUEST_OFFLOADS))
        //         dev->hw_features |= NETIF_F_GRO_HW;

        //     dev->vlan_features = dev->features;

        //     /* MTU range: 68 - 65535 */
        //     dev->min_mtu = MIN_MTU;
        //     dev->max_mtu = MAX_MTU;

        //     /* Configuration may specify what MAC to use.  Otherwise random. */
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_MAC)) {
        //         u8 addr[ETH_ALEN];

        //         virtio_cread_bytes(vdev,
        //                 offsetof(struct virtio_net_config, mac),
        //                 addr, ETH_ALEN);
        //         eth_hw_addr_set(dev, addr);
        //     } else {
        //         eth_hw_addr_random(dev);
        //     }

        //     /* Set up our device-specific information */
        //     vi = netdev_priv(dev);
        //     vi->dev = dev;
        //     vi->vdev = vdev;
        //     vdev->priv = vi;

        //     INIT_WORK(&vi->config_work, virtnet_config_changed_work);

        //     /* If we can receive ANY GSO packets, we must allocate large ones. */
        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_TSO4) ||
        //         virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_TSO6) ||
        //         virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_ECN) ||
        //         virtio_has_feature(vdev, VIRTIO_NET_F_GUEST_UFO))
        //         vi->big_packets = true;

        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_MRG_RXBUF))
        //         vi->mergeable_rx_bufs = true;

        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_HASH_REPORT))
        //         vi->has_rss_hash_report = true;

        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_RSS))
        //         vi->has_rss = true;

        //     if (vi->has_rss || vi->has_rss_hash_report) {
        //         vi->rss_indir_table_size =
        //             virtio_cread16(vdev, offsetof(struct virtio_net_config,
        //                 rss_max_indirection_table_length));
        //         vi->rss_key_size =
        //             virtio_cread8(vdev, offsetof(struct virtio_net_config, rss_max_key_size));

        //         vi->rss_hash_types_supported =
        //             virtio_cread32(vdev, offsetof(struct virtio_net_config, supported_hash_types));
        //         vi->rss_hash_types_supported &=
        //                 ~(VIRTIO_NET_RSS_HASH_TYPE_IP_EX |
        //                 VIRTIO_NET_RSS_HASH_TYPE_TCP_EX |
        //                 VIRTIO_NET_RSS_HASH_TYPE_UDP_EX);

        //         dev->hw_features |= NETIF_F_RXHASH;
        //     }

        //     if (vi->has_rss_hash_report)
        //         vi->hdr_len = sizeof(struct virtio_net_hdr_v1_hash);
        //     else if (virtio_has_feature(vdev, VIRTIO_NET_F_MRG_RXBUF) ||
        //         virtio_has_feature(vdev, VIRTIO_F_VERSION_1))
        //         vi->hdr_len = sizeof(struct virtio_net_hdr_mrg_rxbuf);
        //     else
        //         vi->hdr_len = sizeof(struct virtio_net_hdr);

        //     if (virtio_has_feature(vdev, VIRTIO_F_ANY_LAYOUT) ||
        //         virtio_has_feature(vdev, VIRTIO_F_VERSION_1))
        //         vi->any_header_sg = true;

        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_CTRL_VQ))
        //         vi->has_cvq = true;

        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_MTU)) {
        //         mtu = virtio_cread16(vdev,
        //                     offsetof(struct virtio_net_config,
        //                         mtu));
        //         if (mtu < dev->min_mtu) {
        //             /* Should never trigger: MTU was previously validated
        //             * in virtnet_validate.
        //             */
        //             dev_err(&vdev->dev,
        //                 "device MTU appears to have changed it is now %d < %d",
        //                 mtu, dev->min_mtu);
        //             err = -EINVAL;
        //             goto free;
        //         }

        //         dev->mtu = mtu;
        //         dev->max_mtu = mtu;

        //         /* TODO: size buffers correctly in this case. */
        //         if (dev->mtu > ETH_DATA_LEN)
        //             vi->big_packets = true;
        //     }

        //     if (vi->any_header_sg)
        //         dev->needed_headroom = vi->hdr_len;

        //     /* Enable multiqueue by default */
        //     if (num_online_cpus() >= max_queue_pairs)
        //         vi->curr_queue_pairs = max_queue_pairs;
        //     else
        //         vi->curr_queue_pairs = num_online_cpus();
        //     vi->max_queue_pairs = max_queue_pairs;

        //     /* Allocate/initialize the rx/tx queues, and invoke find_vqs */
        //     err = init_vqs(vi);
        //     if (err)
        //         goto free;

        // #ifdef CONFIG_SYSFS
        //     if (vi->mergeable_rx_bufs)
        //         dev->sysfs_rx_queue_group = &virtio_net_mrg_rx_group;
        // #endif
        //     netif_set_real_num_tx_queues(dev, vi->curr_queue_pairs);
        //     netif_set_real_num_rx_queues(dev, vi->curr_queue_pairs);

        //     virtnet_init_settings(dev);

        //     if (virtio_has_feature(vdev, VIRTIO_NET_F_STANDBY)) {
        //         vi->failover = net_failover_create(vi->dev);
        //         if (IS_ERR(vi->failover)) {
        //             err = PTR_ERR(vi->failover);
        //             goto free_vqs;
        //         }
        //     }

        //     if (vi->has_rss || vi->has_rss_hash_report)
        //         virtnet_init_default_rss(vi);

        //     /* serialize netdev register + virtio_device_ready() with ndo_open() */
        //     rtnl_lock();

        //     err = register_netdevice(dev);
        //     if (err) {
        //         pr_debug("virtio_net: registering device failed\n");
        //         rtnl_unlock();
        //         goto free_failover;
        //     }

        //     virtio_device_ready(vdev);

        //     rtnl_unlock();

        //     err = virtnet_cpu_notif_add(vi);
        //     if (err) {
        //         pr_debug("virtio_net: registering cpu notifier failed\n");
        //         goto free_unregister_netdev;
        //     }

        //     virtnet_set_queues(vi, vi->curr_queue_pairs);

        //     /* Assume link up if device can't report link status,
        //     otherwise get link status from config. */
        //     netif_carrier_off(dev);
        //     if (virtio_has_feature(vi->vdev, VIRTIO_NET_F_STATUS)) {
        //         schedule_work(&vi->config_work);
        //     } else {
        //         vi->status = VIRTIO_NET_S_LINK_UP;
        //         virtnet_update_settings(vi);
        //         netif_carrier_on(dev);
        //     }

        //     for (i = 0; i < ARRAY_SIZE(guest_offloads); i++)
        //         if (virtio_has_feature(vi->vdev, guest_offloads[i]))
        //             set_bit(guest_offloads[i], &vi->guest_offloads);
        //     vi->guest_offloads_capable = vi->guest_offloads;

        //     pr_debug("virtnet: registered device %s with %d RX and TX vq's\n",
        //         dev->name, max_queue_pairs);

        //     return 0;

        // free_unregister_netdev:
        //     virtio_reset_device(vdev);

        //     unregister_netdev(dev);
        // free_failover:
        //     net_failover_destroy(vi->failover);
        // free_vqs:
        //     cancel_delayed_work_sync(&vi->refill);
        //     free_receive_page_frags(vi);
        //     virtnet_del_vqs(vi);
        // free:
        //     free_netdev(dev);
        //     return err;
    }
}
