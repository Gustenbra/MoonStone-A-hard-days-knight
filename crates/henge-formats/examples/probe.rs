fn main() {
    let lib = henge_formats::Library::open("/home/claude/game/Moonstone").unwrap();
    for n in lib.with_extension(&["cel","ob","f","fon"]) {
        let d = lib.bytes(&n).unwrap();
        let count = u16::from_be_bytes([d[0],d[1]]) as usize;
        let hl = count*10+10;
        if hl > d.len() { println!("{n}: BAD count={count} len={}", d.len()); continue; }
        let mut maxw=0usize; let mut maxh=0usize;
        for i in 0..count {
            let b=10+i*10;
            let w=u16::from_be_bytes([d[b+4],d[b+5]]) as usize;
            let h=u16::from_be_bytes([d[b+6],d[b+7]]) as usize;
            maxw=maxw.max(w); maxh=maxh.max(h);
        }
        if maxw>400 || maxh>400 || count>500 {
            println!("{n}: count={count} maxw={maxw} maxh={maxh} filelen={}", d.len());
        }
    }
}
