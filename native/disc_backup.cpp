// Read-only Linux MMC CD backup helper. No write, burn, erase or eject commands.
#include <linux/cdrom.h>
#include <scsi/sg.h>
#include <sys/ioctl.h>
#include <fcntl.h>
#include <unistd.h>
#include <algorithm>
#include <climits>
#include <regex>
#include <array>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
struct Track { int number, lba; bool data; };
struct Disc { std::vector<Track> tracks; int sectors; std::string title, system, token, serial; };
struct Device { int fd; explicit Device(const char* p) : fd(open(p,O_RDONLY|O_NONBLOCK)) {if(fd<0)throw std::runtime_error("Cannot open optical drive: "+std::string(strerror(errno)));} ~Device(){close(fd);} };
std::string text(const unsigned char* b,size_t n) {std::string out;bool space=false;for(size_t i=0;i<n;++i){if(b[i]>=33&&b[i]<127){if(space&&!out.empty())out+=' ';out+=char(b[i]);space=false;}else space=true;}return out;}
std::string quoted(const std::string& s){std::string out="\"";for(unsigned char c:s){if(c=='"'||c=='\\')out+='\\';if(c>=32)out+=char(c);}return out+'"';}
Disc inspect(int fd){
 if(ioctl(fd,CDROM_DRIVE_STATUS,CDSL_CURRENT)!=CDS_DISC_OK)throw std::runtime_error("Insert a game CD and close the tray, then refresh.");
 cdrom_multisession session{};session.addr_format=CDROM_LBA;
 if(ioctl(fd,CDROMMULTISESSION,&session)==0&&session.xa_flag&&session.addr.lba>0)throw std::runtime_error("Multi-session discs are not supported.");
 cdrom_tochdr hdr{};if(ioctl(fd,CDROMREADTOCHDR,&hdr)<0)throw std::runtime_error("Cannot read disc track list.");
 if(hdr.cdth_trk0!=1||hdr.cdth_trk1<1||hdr.cdth_trk1>99)throw std::runtime_error("Unsupported CD track list.");
 Disc disc{};
 for(int n=1;n<=int(hdr.cdth_trk1)+1;++n){cdrom_tocentry e{};e.cdte_track=n>hdr.cdth_trk1?CDROM_LEADOUT:n;e.cdte_format=CDROM_LBA;if(ioctl(fd,CDROMREADTOCENTRY,&e)<0)throw std::runtime_error("Cannot read CD track boundary.");if(n>hdr.cdth_trk1)disc.sectors=e.cdte_addr.lba;else disc.tracks.push_back({n,e.cdte_addr.lba,bool(e.cdte_ctrl&CDROM_DATA_TRACK)});}
 if(disc.tracks.front().lba!=0||!disc.tracks.front().data||disc.sectors<=0||disc.sectors>450000)throw std::runtime_error("Insert a supported game CD with a data track first.");
 for(size_t i=0;i<disc.tracks.size();++i)if(disc.tracks[i].lba<0||disc.tracks[i].lba>=(i+1<disc.tracks.size()?disc.tracks[i+1].lba:disc.sectors))throw std::runtime_error("Invalid CD track boundaries.");
 std::array<unsigned char,2048> b{};if(pread(fd,b.data(),b.size(),0)<512)throw std::runtime_error("Cannot read game header. Clean the disc and retry.");
 std::vector<unsigned char> sample(std::min<size_t>(2*1024*1024,size_t(disc.tracks.size()>1?disc.tracks[1].lba:disc.sectors)*2048));
 ssize_t got=pread(fd,sample.data(),sample.size(),0);if(got>=512)sample.resize(got);else sample.assign(b.begin(),b.end());
 std::string header(reinterpret_cast<char*>(sample.data()),sample.size());
 if(header.find("SEGADISCSYSTEM")!=std::string::npos){disc.system="segacd";disc.serial=text(b.data()+0x180,14);disc.title=text(b.data()+0x150,48);if(disc.title.empty())disc.title=text(b.data()+0x120,48);}
 else if(header.find("SEGA SEGASATURN")!=std::string::npos){disc.system="saturn";disc.serial=text(b.data()+0x20,10);disc.title=text(b.data()+0x60,112);}
 else if(header.find("SEGA SEGAKATANA")!=std::string::npos)throw std::runtime_error("Dreamcast GD-ROMs require specialized dumping hardware.");
 else if(header.find("PLAYSTATION")!=std::string::npos){disc.system="psx";std::smatch match;if(std::regex_search(header,match,std::regex("BOOT\\s*=\\s*cdrom[^\\r\\n]*?([A-Z]{4}[_-][0-9]{3}[.][0-9]{2})",std::regex::icase)))disc.serial=match[1].str();}
 if(disc.title.empty()&&sample.size()>=32840&&std::equal(sample.begin()+32769,sample.begin()+32774,"CD001")){disc.title=text(sample.data()+32808,32);std::replace(disc.title.begin(),disc.title.end(),'_',' ');}
 uint64_t hash=14695981039346656037ULL;auto add=[&](unsigned char c){hash^=c;hash*=1099511628211ULL;};for(size_t i=0;i<std::min<size_t>(sample.size(),65536);++i)add(sample[i]);for(auto t:disc.tracks){for(int j=0;j<4;++j)add((t.lba>>(j*8))&255);add(t.data?1:0);}for(int j=0;j<4;++j)add((disc.sectors>>(j*8))&255);
 std::ostringstream token;token<<std::hex<<hash;disc.token=token.str();return disc;
}
void describe(const Disc& d){std::cout<<"{\"kind\":\"disc\",\"title\":"<<quoted(d.title)<<",\"system\":"<<quoted(d.system)<<",\"serial\":"<<quoted(d.serial)<<",\"token\":"<<quoted(d.token)<<",\"sectors\":"<<d.sectors<<",\"tracks\":"<<d.tracks.size()<<",\"layout\":[";bool comma=false;for(auto t:d.tracks){if(comma)std::cout<<",";comma=true;std::cout<<"{\"number\":"<<t.number<<",\"lba\":"<<t.lba<<",\"data\":"<<(t.data?"true":"false")<<"}";}std::cout<<"]}"<<std::endl;}
std::vector<unsigned char> read_cd(int fd,int lba,int count){
 std::array<unsigned char,12> cdb{};std::array<unsigned char,64> sense{};cdb[0]=0xbe;for(int i=0;i<4;++i)cdb[2+i]=(lba>>((3-i)*8))&255;for(int i=0;i<3;++i)cdb[6+i]=(count>>((2-i)*8))&255;cdb[9]=0xf8;
 std::vector<unsigned char> data(count*2352);sg_io_hdr_t h{};h.interface_id='S';h.dxfer_direction=SG_DXFER_FROM_DEV;h.cmd_len=cdb.size();h.mx_sb_len=sense.size();h.dxfer_len=data.size();h.dxferp=data.data();h.cmdp=cdb.data();h.sbp=sense.data();h.timeout=10000;
 if(ioctl(fd,SG_IO,&h)<0)throw std::runtime_error("READ CD ioctl failed: "+std::string(strerror(errno)));
 if(h.status||h.host_status||h.driver_status||h.resid||(h.info&SG_INFO_OK_MASK)!=SG_INFO_OK)throw std::runtime_error("READ CD failed at LBA "+std::to_string(lba)+" (drive status "+std::to_string(h.status)+", host "+std::to_string(h.host_status)+", residual "+std::to_string(h.resid)+"). No complete backup was produced.");
 return data;
}
std::string msf(int lba){std::ostringstream s;s<<std::setfill('0')<<std::setw(2)<<lba/4500<<":"<<std::setw(2)<<(lba/75)%60<<":"<<std::setw(2)<<lba%75;return s.str();}
void backup(int fd,const Disc& d,const std::string& folder){
 // Staging must be newly created by the frontend; refuse existing image files.
 std::string bin=folder+"/disc.bin",cue=folder+"/disc.cue";
 int out=open(bin.c_str(),O_WRONLY|O_CREAT|O_EXCL,0600);if(out<0)throw std::runtime_error("Cannot create staged image.");
 try{for(int lba=0;lba<d.sectors;lba+=16){int count=std::min(16,d.sectors-lba);std::vector<unsigned char> bytes;
 try{bytes=read_cd(fd,lba,count);}catch(const std::runtime_error&){for(int i=0;i<count;++i){auto part=read_cd(fd,lba+i,1);bytes.insert(bytes.end(),part.begin(),part.end());}}
 size_t offset=0;while(offset<bytes.size()){ssize_t n=write(out,bytes.data()+offset,bytes.size()-offset);if(n<=0)throw std::runtime_error("Cannot write image (check free disk space).");offset+=n;}
 if(lba%256==0||lba+count==d.sectors)std::cout<<"{\"kind\":\"progress\",\"done\":"<<uint64_t(lba+count)*2352<<",\"total\":"<<uint64_t(d.sectors)*2352<<"}"<<std::endl;
 }if(fsync(out)<0)throw std::runtime_error("Cannot flush disc image.");close(out);out=-1;
 if(inspect(fd).token!=d.token)throw std::runtime_error("The disc changed during backup.");
 std::ifstream input(bin,std::ios::binary);std::ostringstream lines;lines<<"FILE \"disc.bin\" BINARY\n";
 for(auto t:d.tracks){std::string mode="AUDIO";if(t.data){std::array<unsigned char,2352>b{};input.seekg(uint64_t(t.lba)*2352);input.read(reinterpret_cast<char*>(b.data()),b.size());bool sync=b[0]==0&&b[11]==0;for(int j=1;j<11;++j)sync=sync&&b[j]==255;if(!input||!sync||(b[15]!=1&&b[15]!=2))throw std::runtime_error("Unsupported raw data sector returned by drive.");mode="MODE"+std::to_string(b[15])+"/2352";}lines<<"  TRACK "<<std::setfill('0')<<std::setw(2)<<t.number<<" "<<mode<<"\n    INDEX 01 "<<msf(t.lba)<<"\n";}
 int cuefd=open(cue.c_str(),O_WRONLY|O_CREAT|O_EXCL,0600);if(cuefd<0)throw std::runtime_error("Cannot create staged CUE.");auto s=lines.str();ssize_t n=write(cuefd,s.data(),s.size());int flushed=fsync(cuefd);close(cuefd);if(n!=ssize_t(s.size())||flushed<0)throw std::runtime_error("Cannot write complete CUE sheet.");
 std::cout<<"{\"kind\":\"complete\"}"<<std::endl;
 }catch(...){if(out>=0)close(out);throw;}
}
int main(int argc,char** argv){try{
 if(argc==2&&std::string(argv[1])=="--self-test"){if(msf(0)!="00:00:00"||msf(11159)!="02:28:59"||quoted("a\"b")!="\"a\\\"b\"")return 1;std::cout<<"MMC layout formatting passed"<<std::endl;return 0;}
 if(argc!=3&&argc!=5)throw std::runtime_error("Usage: emuluna-disc-backup status DEVICE | inspect DEVICE | backup DEVICE STAGING EXPECTED_TOKEN");
 std::string mode=argv[1];if(((mode=="inspect"||mode=="status")&&argc!=3)||(mode=="backup"&&argc!=5)||(mode!="inspect"&&mode!="status"&&mode!="backup"))throw std::runtime_error("Invalid operation.");
 std::string dev=argv[2];if(dev.find("/dev/sr")!=0||dev.size()<=7||dev.find_first_not_of("0123456789",7)!=std::string::npos)throw std::runtime_error("Select a Linux optical /dev/sr device.");
 Device device(argv[2]);
 if (mode == "status") {
     const int status = ioctl(device.fd, CDROM_DRIVE_STATUS, CDSL_CURRENT);
     if (status < 0 && errno != ENOMEDIUM)
         throw std::runtime_error("Cannot check optical drive: " + std::string(strerror(errno)));
     std::cout << (status == CDS_DISC_OK ? "ready" : "waiting") << std::endl;
     return 0;
 }
 Disc disc=inspect(device.fd);if(mode=="inspect")describe(disc);else{if(disc.token!=argv[4])throw std::runtime_error("Disc changed. Refresh before backing up.");backup(device.fd,disc,argv[3]);}return 0;
 }catch(const std::exception& e){std::cerr<<e.what()<<std::endl;return 1;}}
