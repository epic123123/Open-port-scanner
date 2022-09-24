use std::net::SocketAddr;
use std::net::{Shutdown, TcpStream};
use std::time::{Instant/*, Duration*/};
use std::thread;
use std::env;
use std::sync::{Arc, mpsc, Mutex};

const MAX_NUM_OF_LOCAL_THREADS: usize = 50;
const AMOUNT_OF_TRIES: u8 = 1;

struct ThreadPool
{
    threads: Vec<Thread>,
    ports_per_local_thread: usize,
    addr: Vec<SocketAddr>,
    open_list: Arc<Mutex<Vec<u16>>>

}
struct Thread {
    handle: std::thread::JoinHandle<()>
}

impl ThreadPool
{
    fn new(num: usize, high: usize, low: usize, addr: Vec<SocketAddr>, open_list_: Arc<Mutex<Vec<u16>>>) -> ThreadPool
    {
        ThreadPool {
            threads: Vec::with_capacity(num),
            ports_per_local_thread: (high - low) as usize / num,
            addr: addr,
            open_list: open_list_
        }
    }

    fn scan(mut self, addrs: Vec<SocketAddr>, thread_id: u8) -> ThreadPool
    {
        let open_list = Arc::clone(&self.open_list);

        self.threads.push(Thread {
            handle: thread::spawn(move || {
                for addr in addrs
                {
                    if try_conn(addr, AMOUNT_OF_TRIES)
                    {
                        println!("Found open port {} from scanner thread {}", addr.port(), thread_id);
                        let mut op = open_list.lock().unwrap();
                        op.push(addr.port());
                    }
                }  
            })
        });

        return self;
    }
    fn scan_all(mut self) -> ThreadPool
    {
        let add = self.addr.clone();
        let addr_chunks = add.chunks(self.ports_per_local_thread);

        let mut thread_id: u8 = 0;

        for addrs in addr_chunks
        {  
            self = self.scan(addrs.to_vec(), thread_id);
            thread_id = thread_id + 1;
        }

        return self;
    }

    fn join_all(self)
    {
        for item in self.threads
        {
            item.handle.join().unwrap();
        }
    }
}


fn try_conn(ip: SocketAddr, tries: u8) -> bool
{
    for _ in 0..tries
    {
        //let conn = TcpStream::connect_timeout(&ip, Duration::from_millis(150));
        let conn = TcpStream::connect(&ip);
        match conn
        {
            Ok(r) => {
                TcpStream::shutdown(&r, Shutdown::Both).unwrap();

                return true;
            },
            Err(_) => {
                continue;
            }
        }
    }

    return false;
}

fn to_sock_addr(target_base: String) -> Result<[u8; 4], &'static str>
{
    let mut target_ip: [u8; 4] = [0, 0, 0, 0];

    let ip = target_base.split('.');

    let mut i: usize = 0;

    for s in ip
    {
        if let Ok(r) = s.parse::<u8>()
        {
            target_ip[i] = r;
        }
        else
        {
            eprintln!("Error when making IP a Socket Address.");
        }

        i = i + 1;
    }

    return Ok(target_ip);

}

fn arg_check(args: &Vec<String>) -> Result<Vec<u16>, &'static str> // 0 = low, 1 = high, 2 = thread num
{
    if args.len() != 9
    {
        println!("Not Enough Arguments: Usage Scanner -l <0> -h <1000> -t <4>\n-l = LOW | -h = HIGH | -IP = ip address of target\n(ex. so the range would be 0 to 1000)\n -t = NUM OF THREADS");
        return Err("Bad number");
    }

    let mut i: usize = 0;

    let mut info: Vec<u16> = vec![];

    loop
    {
        if args[i].eq("-l")
        {
            info.push(args[i+1].parse::<u16>().unwrap());
        }
        if args[i].eq("-h")
        {
            info.push(args[i+1].parse::<u16>().unwrap());
        }
        if args[i].eq("-t")
        {
            info.push(args[i+1].parse::<u16>().unwrap());
        }

        i = i + 1;

        if i >= args.len()
        {
            break;
        }
    }

    return Ok(info);
}

fn main() {
    println!("-------------------\nOPEN PORT SCANNER\n-------------------");

    let args: Vec<String> = env::args().collect();

    let result = arg_check(&args);

    if let Err(_) = result
    {
        println!("Exiting...");
        return;
    }

    let mut target_ip_base: String = String::new();

    for i in 0..args.len() // Grab ip because vec can't hold it
    {
        if args[i].eq("-IP")
        {
            target_ip_base = String::from(&args[i+1]);
            break;
        }
    }

    let info = result.unwrap();

    println!("Starting head threads: {}", info[2]);
    println!("Starting scanner threads: {}", info[2] * MAX_NUM_OF_LOCAL_THREADS as u16);

    let mut handles: Vec<std::thread::JoinHandle<()>> = vec![];
    let mut receivers = vec![];

    let ports_per_each_thread = (info[1] - info[0]) / info[2];

    println!("Low: {} - High: {} - Ports per main thread: {} - Ports per scanner thread: {}", info[0], info[1], ports_per_each_thread, 
        ports_per_each_thread / MAX_NUM_OF_LOCAL_THREADS as u16);

    let a = match to_sock_addr(target_ip_base.clone())
    {
        Ok(a) => a,
        Err(_) => {return;}
    };

    let now = Instant::now();

    for i in 0..info[2]
    {
        let low = info[0] + i * ports_per_each_thread;
        let high = low + ports_per_each_thread;
        
        let (tx, rx) = mpsc::channel();

        let (open_tx, open_rx) = mpsc::channel();

        if let Err(e) = tx.send(a.clone())
        {
            println!("Error occurred when sending data to channel: {e:?}");
            return;
        }

        receivers.push(open_rx);

        handles.push(thread::spawn( move || {
            
            let open = Arc::new(Mutex::new(vec![]));

            let target_ip = rx.recv().unwrap();

            let mut socketaddrs: Vec<SocketAddr> = Vec::with_capacity((high - low) as usize);

            for i in low..high
            {
                socketaddrs.push(SocketAddr::from((target_ip, i)));
            }

            let open_a = Arc::clone(&open);

            let mut threadpool = ThreadPool::new(MAX_NUM_OF_LOCAL_THREADS, high as usize, low as usize, socketaddrs.clone(), open_a);

            threadpool = threadpool.scan_all();

            threadpool.join_all();

            open_tx.send(open).unwrap();
            
        }));
    }

    let mut open_port_vectors: Vec<Vec<u16>> = vec![];

    let mut _all_threads_finished = true;

    for item in handles
    {
        item.join().unwrap();
    }

    for i in 0..receivers.len()
    {
            match receivers[i].try_recv()
            {
                Ok(t) => {
                    let data = t.lock().unwrap();
                    open_port_vectors.push(data.to_vec())
                },
                Err(_) => _all_threads_finished = false
            }
    }

    println!("Threads finished");

    println!("Open ports:");

    for vec in open_port_vectors
    {
        for port in vec
        {
            print!("{} (TCP), ", port);
        }
    }

    println!("\nScan took {} seconds", now.elapsed().as_secs());

}
