// @generated por tools/palettes/gen_devices.py desde los .tech de Magic. No editar.
//! Reglas de transistores de cada PDK para cuando no está instalado: lo que
//! `rules.rs` lee del `.tech` (primer estilo de `cifinput`, líneas `device`).

/// `sky130A/libs.tech/magic/sky130A.tech`: 57 líneas `device` de MOS.
pub const SKY130: &str = r#"types
dwell dnwell,dnw
dwell isosubstrate,isosub
dwell photodiode,photo
well nwell,nw
well pwell,pw
well rpw,rpwell
well pbase,npn
well nbase,pnp
active nmos,ntransistor,nfet
-active scnmos,scntransistor,scnfet
-active npd,npdfet,sramnfet
-active npass,npassfet,srampassfet
active pmos,ptransistor,pfet
-active scpmos,scptransistor,scpfet
-active scpmoshvt,scpfethvt
-active ppu,ppufet,srampfet
active nnmos,nntransistor,nnfet
active mvnmos,mvntransistor,mvnfet
active mvpmos,mvptransistor,mvpfet
active mvnnmos,mvnntransistor,mvnnfet
-active mvnmosesd,mvntransistoresd,mvnfetesd
-active mvpmosesd,mvptransistoresd,mvpfetesd
active varactor,varact,var
active mvvaractor,mvvaract,mvvar
active pmoslvt,pfetlvt
active pmosmvt,pfetmvt
active pmoshvt,pfethvt
active nmoslvt,nfetlvt
active varactorhvt,varacthvt,varhvt
-active nsonos,sonos
-active sramnvar,corenvar,corenvaractor
-active srampvar,corepvar,corepvaractor
active ndiff,ndiffusion,ndif
active pdiff,pdiffusion,pdif
active mvndiff,mvndiffusion,mvndif
active mvpdiff,mvpdiffusion,mvpdif
active ndiffc,ndcontact,ndc
active pdiffc,pdcontact,pdc
active mvndiffc,mvndcontact,mvndc
active mvpdiffc,mvpdcontact,mvpdc
active psubdiff,psubstratepdiff,ppdiff,ppd,psd,ptap
active nsubdiff,nsubstratendiff,nndiff,nnd,nsd,ntap
active mvpsubdiff,mvpsubstratepdiff,mvppdiff,mvppd,mvpsd,mvptap
active mvnsubdiff,mvnsubstratendiff,mvnndiff,mvnnd,mvnsd,mvntap
active psubdiffcont,psubstratepcontact,psc,ptapc
active nsubdiffcont,nsubstratencontact,nsc,ntapc
active mvpsubdiffcont,mvpsubstratepcontact,mvpsc,mvptapc
active mvnsubdiffcont,mvnsubstratencontact,mvnsc,mvntapc
active extdrain,ed
active poly,p,polysilicon
active polycont,pc,pcontact,polycut,polyc
active xpolycontact,xpolyc,xpc
active npolyres,npres,mrp1
active ppolyres,ppres,xhrpoly
active xpolyres,xpres,xres,uhrpoly
active ndiffres,rnd,rdn,rndiff
active pdiffres,rpd,rdp,rpdiff
active mvndiffres,mvrnd,mvrdn,mvrndiff
active mvpdiffres,mvrpd,mvrdp,mvrpdiff
active pdiode,pdi
active ndiode,ndi
active nndiode,nndi
active pdiodec,pdic
active ndiodec,ndic
active nndiodec,nndic
active mvpdiode,mvpdi
active mvndiode,mvndi
active mvpdiodec,mvpdic
active mvndiodec,mvndic
active pdiodelvt,pdilvt
active pdiodehvt,pdihvt
active ndiodelvt,ndilvt
active pdiodelvtc,pdilvtc
active pdiodehvtc,pdihvtc
active ndiodelvtc,ndilvtc
locali locali,li1,li
-locali corelocali,coreli1,coreli
locali rlocali,rli1,rli
locali viali,vial,mcon,m1c,v0
-locali obsli1,obsli
-locali obsli1c,obsmcon
metal1 metal1,m1,met1
metal1 rmetal1,rm1,rmet1
metal1 via1,m2contact,m2cut,m2c,via,v,v1
metal2 metal2,m2,met2
metal2 rmetal2,rm2,rmet2
metal2 via2,m3contact,m3cut,m3c,v2
metal3 metal3,m3,met3
metal3 rmetal3,rm3,rmet3
metal3 via3,v3
cap1 mimcap,mim,capm
cap1 mimcapcontact,mimcapc,mimcc,capmc
metal4 metal4,m4,met4
metal4 rmetal4,rm4,rmet4
metal4 via4,v4
cap2 mimcap2,mim2,capm2
cap2 mimcap2contact,mimcap2c,mim2cc,capm2c
metal5 metal5,m5,met5
metal5 rm5,rmetal5,rmet5
metal5 mrdlcontact,mrdlc,pi1
metali metalrdl,mrdl,metrdl,rdl
end
cifinput
style riku
layer pnp NWELL,WELLTXT,WELLPIN
and PNPID
layer nwell NWELL,WELLTXT,WELLPIN
and-not PNPID
templayer nwellarea NWELL
copyup nwelcheck
templayer xnwelcheck nwelcheck
copyup nwelcheck
templayer hvarea HVI
copyup hvcheck
templayer xhvcheck hvcheck
copyup hvcheck
layer pwell TAP,DIFF
and-not NWELL,nwelcheck
grow 130
or SUBTXT,SUBPIN
grow 420
shrink 420
layer dnwell DNWELL
layer isosub SUBCUT
layer npn DNWELL
and-not NWELL,nwelcheck
and NPNID
layer photo DNWELL
and PHOTO
layer rpw PWRES
and DNWELL
templayer ndiffarea DIFF,DIFFTXT,DIFFPIN,barediff
and-not POLY
and-not NWELL,nwelcheck
and-not PSDM
and-not DIODE
and-not DIFFRES
and-not HVI,hvcheck
and NSDM
and-not CORELI
copyup ndifcheck
layer ndiff ndiffarea
templayer xndifcheck ndifcheck
copyup ndifcheck
templayer mvndiffarea DIFF,DIFFTXT,DIFFPIN,barediff
and-not POLY
and-not NWELL,nwelcheck
and-not PSDM
and-not DIODE
and-not DIFFRES
and HVI,hvcheck
and NSDM
copyup ndifcheck
layer mvndiff mvndiffarea
templayer mvxndifcheck mvndifcheck
copyup mvndifcheck
layer ndiode DIFF,barediff
and NSDM
and DIODE
and-not NWELL,nwelcheck
and-not POLY
and-not PSDM
and-not HVI,hvcheck
and-not LVTN
layer ndiodelvt DIFF,barediff
and NSDM
and DIODE
and-not NWELL,nwelcheck
and-not POLY
and-not PSDM
and-not HVI,hvcheck
and LVTN
templayer ndiodearea DIODE
and NSDM
and-not HVI,hvcheck
and-not NWELL,nwelcheck
copyup DIODE,NSDM
layer ndiffres DIFFRES
and NSDM
and-not HVI,hvcheck
templayer pdiffarea DIFF,DIFFTXT,DIFFPIN,barediff
and-not POLY
and NWELL,nwelcheck
and-not NSDM
and-not DIODE
and-not HVI,hvcheck
and PSDM
copyup pdifcheck
layer pdiff pdiffarea
layer mvndiode DIFF,barediff
and NSDM
and DIODE
and HVI,hvcheck
and-not POLY
and-not PSDM
and-not LVTN
layer nndiode DIFF,barediff
and NSDM
and DIODE
and HVI,hvcheck
and-not POLY
and-not PSDM
and LVTN
templayer mvndiodearea DIODE
and NSDM
and HVI,hvcheck
and-not NWELL,nwelcheck
copyup DIODE,NSDM
layer mvndiffres DIFFRES
and NSDM
and HVI,hvcheck
templayer mvpdiffarea DIFF,DIFFTXT,DIFFPIN,barediff
and-not POLY
and NWELL,nwelcheck
and-not NSDM
and HVI,hvcheck
and-not DIODE
and-not DIFFRES
and PSDM
copyup mvpdifcheck
layer mvpdiff mvpdiffarea
templayer xpdifcheck pdifcheck
copyup pdifcheck
layer pdiode DIFF,barediff
and PSDM
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and-not LVTN
and-not HVTP
and DIODE
layer pdiodelvt DIFF,barediff
and PSDM
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and LVTN
and-not HVTP
and DIODE
layer pdiodehvt DIFF,barediff
and PSDM
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and-not LVTN
and HVTP
and DIODE
templayer pdiodearea DIODE
and PSDM
and-not HVI,hvcheck
copyup DIODE,PSDM
templayer pfetarea DIFF,barediff
and POLY
or baretrans
and-not NSDM
and-not HVI,hvcheck
layer pfet pfetarea
and-not LVTN
and-not HVTP
and-not STDCELL
and-not COREID
layer scpfet pfetarea
and-not LVTN
and-not HVTP
and STDCELL
and-not COREID
layer scpfethvt pfetarea
and-not LVTN
and HVTP
and STDCELL
layer ppu pfetarea
and-not LVTN
and HVTP
and COREID
layer pfetlvt pfetarea
and LVTN
layer pfetmvt pfetarea
and HVTR
layer pfethvt pfetarea
and HVTP
and-not STDCELL
and-not COREID
layer nwell pfetarea
and-not COREID
grow 180
templayer mvxpdifcheck mvpdifcheck
copyup mvpdifcheck
layer mvpdiode DIFF,barediff
and PSDM
and-not POLY
and-not NSDM
and HVI,hvcheck
and DIODE
templayer mvpdiodearea DIODE
and PSDM
and HVI,hvcheck
copyup DIODE,PSDM
templayer mvpfetarea DIFF,barediff
and POLY
or baretrans
and-not NSDM
and HVI,hvcheck
layer mvpfet mvpfetarea
and-not ESDID
layer mvpfetesd mvpfetarea
and ESDID
layer pdiff DIFF,DIFFTXT,DIFFPIN,barediff
and-not NSDM
and-not POLY
and-not HVI,hvcheck
and-not DIODE
and-not DIFFRES
layer pdiffres DIFFRES
and PSDM
and NWELL,nwelcheck
and-not HVI,hvcheck
layer nfet DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not HVI,hvcheck
and-not LVTN
and-not SONOS
and-not STDCELL
and-not COREID
layer scnfet DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not NWELL,nwelcheck
and-not HVI,hvcheck
and-not LVTN
and-not SONOS
and STDCELL
layer npass DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not NWELL,nwelcheck
and COREID
layer npd DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not NWELL,nwelcheck
and COREID
shrink 70
grow 70
layer npd TAP
grow 100
and DIFF
and POLY
and-not PSDM
and NSDM
and-not NWELL,nwelcheck
and COREID
layer nfetlvt DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not HVI,hvcheck
and LVTN
and-not SONOS
layer nsonos DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not HVI,hvcheck
and LVTN
and SONOS
templayer nsdarea TAP,DIFF
and NSDM
and NWELL,nwelcheck
and-not POLY
and-not PSDM
and-not HVI,hvcheck
and-not CORELI
copyup nsubcheck
layer nsd nsdarea
layer nsd TAP,TAPTXT
and NSDM
and-not POLY
and-not HVI,hvcheck
layer corenvar TAP
and NSDM
and POLY
and COREID
templayer nsdexpand nsdarea
grow 500
templayer xnsubcheck nsubcheck
copyup nsubcheck
templayer psdarea TAP,DIFF
and PSDM
and-not NWELL,nwelcheck
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and-not pfetexpand
copyup psubcheck
layer psd psdarea
layer psd TAP
and PSDM
and-not POLY
and-not HVI,hvcheck
layer corepvar TAP
and PSDM
and POLY
and COREID
templayer psdexpand psdarea
grow 500
layer mvpdiff DIFF,DIFFTXT,DIFFPIN,barediff
and-not NSDM
and-not POLY
and HVI,hvcheck
and mvpfetexpand
layer mvpdiffres DIFFRES
and PSDM
and NWELL,nwelcheck
and HVI,hvcheck
and-not mvrdpioedge
templayer mvnfetarea DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and-not LVTN
and HVI,hvcheck
grow 350
templayer mvnnfetarea DIFF,TAP,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and LVTN
and HVI,hvcheck
and-not mvnfetarea
layer mvnfetesd DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and HVI,hvcheck
and ESDID
and-not mvnnfetarea
layer mvnfet DIFF,barediff
and POLY
or baretrans
and-not PSDM
and NSDM
and HVI,hvcheck
and-not ESDID
and-not mvnnfetarea
layer nnfet mvnnfetarea
and LVID
layer mvnnfet mvnnfetarea
and-not LVID
templayer mvnsdarea TAP,DIFF
and NSDM
and NWELL,nwelcheck
and-not POLY
and-not PSDM
and HVI,hvcheck
copyup mvnsubcheck
layer mvnsd mvnsdarea
layer mvnsd TAP,TAPTXT
and NSDM
and HVI,hvcheck
layer mvpfet EDID
and POLY
and-not DIFF
and-not TAP
and-not NWELL
layer mvnfet EDID
and POLY
and-not DIFF
and-not TAP
and NWELL
templayer ldmos_nwell EDID
grow 1200
and NWELL
layer nwell EDID
and POLY
and DIFF
and PSDM
grow 685
or ldmos_nwell
grow 420
shrink 420
layer pwell EDID
and POLY
and DIFF
and NSDM
grow 660
grow 420
shrink 420
layer ed EDID
and-not POLY
and-not DIFF
and-not TAP
templayer mvnsdexpand mvnsdarea
grow 500
templayer mvxnsubcheck mvnsubcheck
copyup mvnsubcheck
templayer mvpsdarea TAP,DIFF,barediff
and PSDM
and-not NWELL,nwelcheck
and-not POLY
and-not NSDM
and HVI,hvcheck
and-not mvpfetexpand
copyup mvpsubcheck
layer mvpsd mvpsdarea
layer mvpsd TAP,TAPTXT
and PSDM
and HVI,hvcheck
templayer mvpsdexpand mvpsdarea
grow 500
templayer xpsubcheck psubcheck
copyup psubcheck
templayer mvxpsubcheck mvpsubcheck
copyup mvpsubcheck
layer psd TAP
and-not PSDM
and-not NSDM
and-not POLY
and-not HVI,hvcheck
and-not pfetexpand
and psdexpand
layer nsd TAP
and-not PSDM
and-not NSDM
and-not POLY
and-not HVI,hvcheck
and nsdexpand
layer mvpsd TAP
and-not PSDM
and-not NSDM
and-not POLY
and HVI,hvcheck
and-not mvpfetexpand
and mvpsdexpand
layer mvnsd TAP
and-not PSDM
and-not NSDM
and-not POLY
and HVI,hvcheck
and mvnsdexpand
templayer diffresarea DIFFRES
and-not HVI,hvcheck
grow 3000
layer pfet DIFF
and diffresarea
and POLY
and-not NSDM
and-not STDCELL
and-not HVI
layer mvpfet DIFF
and diffresarea
and POLY
and-not NSDM
and-not STDCELL
and HVI
layer scpfet STDCELL
and POLY
and diffresarea
and DIFF
and-not NSDM
and-not HVTP
layer scpfethvt STDCELL
and POLY
and diffresarea
and DIFF
and-not NSDM
and HVTP
templayer xpolyterm RPM,URPM
and POLY
and-not POLYRES
grow 80
and POLY
layer xpc xpolyterm
templayer polyarea POLY,POLYTXT,POLYPIN
and-not POLYRES
and-not POLYSHORT
and-not DIFF
and-not TAP
and-not RPM
and-not URPM
templayer polycontarea polyarea
shrink 130
grow 130
copyup polycheck
layer poly polyarea
templayer xpolycheck polycheck
copyup polycheck
layer mrp1 POLYRES
and POLY
and-not RPM
and-not URPM
layer rmp POLYSHORT
and POLY
layer xhrpoly RPM
and POLYRES
and POLY
and-not URPM
and PSDM
and NPC
and-not xpolyterm
layer uhrpoly URPM
and POLYRES
and POLY
and-not RPM
and NPC
and-not xpolyterm
templayer ndcbase CONT
or barecont
and LI
or barelicont
and DIFF
and NSDM
and-not NWELL,nwelcheck
and-not HVI,hvcheck
layer ndc ndcbase
grow 85
shrink 85
shrink 85
grow 85
or ndcbase
templayer nscbase CONT
or barecont
and LI
or barelicont
and DIFF,TAP
and NSDM
and NWELL,nwelcheck
and-not HVI,hvcheck
layer nsc nscbase
grow 85
shrink 85
shrink 85
grow 85
or nscbase
templayer pdcbase CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and NWELL,nwelcheck
and-not HVI,hvcheck
layer pdc pdcbase
grow 85
shrink 85
shrink 85
grow 85
or pdcbase
templayer pdcnowell CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and pfetexpand
and-not HVI,hvcheck
layer pdc pdcnowell
grow 85
shrink 85
shrink 85
grow 85
or pdcnowell
templayer pscbase CONT
or barecont
and LI
or barelicont
and DIFF,TAP
and PSDM
and-not NWELL,nwelcheck
and-not pfetexpand
and-not HVI,hvcheck
layer psc pscbase
grow 85
shrink 85
shrink 85
grow 85
or pscbase
templayer pcbase CONT
or barecont
and LI
or barelicont
and POLY
and-not DIFF
and-not RPM,URPM
layer pc pcbase
grow 85
shrink 85
shrink 85
grow 85
or pcbase
templayer ndicbase CONT
or barecont
and LI
or barelicont
and DIFF
and NSDM
and DIODE
and-not NWELL,nwelcheck
and-not POLY
and-not PSDM
and-not HVI,hvcheck
and-not LVTN
layer ndic ndicbase
grow 85
shrink 85
shrink 85
grow 85
or ndicbase
templayer ndilvtcbase CONT
or barecont
and LI
or barelicont
and DIFF
and NSDM
and DIODE
and-not NWELL,nwelcheck
and-not POLY
and-not PSDM
and-not HVI,hvcheck
and LVTN
layer ndilvtc ndilvtcbase
grow 85
shrink 85
shrink 85
grow 85
or ndilvtcbase
templayer pdicbase CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and DIODE
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and-not LVTN
and-not HVTP
layer pdic pdicbase
grow 85
shrink 85
shrink 85
grow 85
or pdicbase
templayer pdilvtcbase CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and DIODE
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and LVTN
and-not HVTP
layer pdilvtc pdilvtcbase
grow 85
shrink 85
shrink 85
grow 85
or pdilvtcbase
templayer pdihvtcbase CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and DIODE
and-not POLY
and-not NSDM
and-not HVI,hvcheck
and-not LVTN
and HVTP
layer pdihvtc pdihvtcbase
grow 85
shrink 85
shrink 85
grow 85
or pdihvtcbase
templayer mvndcbase CONT
or barecont
and LI
or barelicont
and DIFF
and NSDM
and-not NWELL,nwelcheck
and HVI,hvcheck
layer mvndc mvndcbase
grow 85
shrink 85
shrink 85
grow 85
or mvndcbase
templayer mvnscbase CONT
or barecont
and LI
or barelicont
and DIFF,TAP
and NSDM
and NWELL,nwelcheck
and HVI,hvcheck
layer mvnsc mvnscbase
grow 85
shrink 85
shrink 85
grow 85
or mvnscbase
templayer mvpdcbase CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and NWELL,nwelcheck
and HVI,hvcheck
layer mvpdc mvpdcbase
grow 85
shrink 85
shrink 85
grow 85
or mvpdcbase
templayer mvpdcnowell CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and mvpfetexpand
and MET1
and HVI,hvcheck
layer mvpdc mvpdcnowell
grow 85
shrink 85
shrink 85
grow 85
or mvpdcnowell
templayer mvpscbase CONT
or barecont
and LI
or barelicont
and DIFF,TAP
and PSDM
and-not NWELL,nwelcheck
and-not mvpfetexpand
and HVI,hvcheck
layer mvpsc mvpscbase
grow 85
shrink 85
shrink 85
grow 85
or mvpscbase
templayer mvndicbase CONT
or barecont
and LI
or barelicont
and DIFF
and NSDM
and DIODE
and-not POLY
and-not PSDM
and-not LVTN
and HVI,hvcheck
layer mvndic mvndicbase
grow 85
shrink 85
shrink 85
grow 85
or mvndicbase
templayer nndicbase CONT
or barecont
and LI
or barelicont
and DIFF
and NSDM
and DIODE
and-not POLY
and-not PSDM
and LVTN
and HVI,hvcheck
layer nndic nndicbase
grow 85
shrink 85
shrink 85
grow 85
or nndicbase
templayer mvpdicbase CONT
or barecont
and LI
or barelicont
and DIFF
and PSDM
and DIODE
and-not POLY
and-not NSDM
and HVI,hvcheck
layer mvpdic mvpdicbase
grow 85
shrink 85
shrink 85
grow 85
or mvpdicbase
layer	fomfill  FOMFILL
layer	polyfill POLYFILL
layer coreli LI,LITXT,LIPIN
and-not LIRES,LISHORT
and COREID
layer locali LI,LITXT,LIPIN
and-not LIRES,LISHORT
and-not COREID
layer rli LI
and LIRES,LISHORT
layer	lifill LIFILL
layer mcon MCON
grow 95
shrink 95
shrink 85
grow 85
or MCON
layer m1 MET1,MET1TXT,MET1PIN
and-not MET1RES,MET1SHORT
layer rm1 MET1
and MET1RES,MET1SHORT
layer m1fill MET1FILL
layer mimcap MET3
and CAPM
layer mimcc VIA3
and CAPM
grow 60
grow 40
shrink 40
layer mimcap2 MET4
and CAPM2
layer mim2cc VIA4
and CAPM2
grow 190
grow 210
shrink 210
templayer m2cbase VIA1
and-not COREID
grow 5
or VIA1
grow 50
layer m2c m2cbase
grow 30
shrink 30
shrink 130
grow 130
or m2cbase
layer m2 MET2,MET2TXT,MET2PIN
and-not MET2RES,MET2SHORT
layer rm2 MET2
and MET2RES,MET2SHORT
layer m2fill MET2FILL
templayer m3cbase VIA2
grow 40
layer m3c m3cbase
grow 60
shrink 60
shrink 140
grow 140
or m3cbase
layer m3 MET3,MET3TXT,MET3PIN
and-not MET3RES,MET3SHORT
layer rm3 MET3
and MET3RES,MET3SHORT
layer m3fill MET3FILL
templayer via3base VIA3
and-not CAPM
grow 60
layer via3 via3base
grow 40
shrink 40
shrink 160
grow 160
or via3base
layer m4 MET4,MET4TXT,MET4PIN
and-not MET4RES,MET4SHORT
layer rm4 MET4
and MET4RES,MET4SHORT
layer m4fill MET4FILL
layer m5 MET5,MET5TXT,MET5PIN
and-not MET5RES,MET5SHORT
layer rm5 MET5
and MET5RES,MET5SHORT
layer m5fill MET5FILL
templayer via4base VIA4
and-not CAPM2
grow 190
layer via4 via4base
grow 210
shrink 210
shrink 590
grow 590
or via4base
layer metrdl RDL,RDLTXT,RDLPIN
templayer gentrans DIFF
and-not PSDM
and-not NSDM
and POLY
copyup baretrans
templayer gendiff DIFF,TAP
and-not PSDM
and-not NSDM
and-not POLY
and-not COREID
copyup barediff
templayer ndiccopy CONT
and LI
and DIODE
and DIFF
and-not NWELL,nwelcheck
and NSDM
and-not LVTN
and-not HVI,hvcheck
layer ndic ndiccopy
grow 85
shrink 85
shrink 85
grow 85
or ndiccopy
templayer mvndiccopy CONT
and LI
and DIODE
and DIFF
and-not NWELL,nwelcheck
and NSDM
and-not LVTN
and HVI,hvcheck
layer mvndic mvndiccopy
grow 85
shrink 85
shrink 85
grow 85
or mvndiccopy
templayer pdiccopy CONT
and LI
and DIODE
and DIFF
and PSDM
and-not HVI,hvcheck
layer pdic pdiccopy
grow 85
shrink 85
shrink 85
grow 85
or pdiccopy
templayer mvpdiccopy CONT
and LI
and DIODE
and PSDM
and HVI,hvcheck
layer mvpdic mvpdiccopy
grow 85
shrink 85
shrink 85
grow 85
or mvpdiccopy
templayer ndccopy CONT
and ndifcheck
layer ndc ndccopy
grow 85
shrink 85
shrink 85
grow 85
or ndccopy
templayer mvndccopy CONT
and mvndifcheck
layer mvndc mvndccopy
grow 85
shrink 85
shrink 85
grow 85
or mvndccopy
templayer pdccopy CONT
and pdifcheck
layer pdc pdccopy
grow 85
shrink 85
shrink 85
grow 85
or pdccopy
templayer mvpdccopy CONT
and mvpdifcheck
layer mvpdc mvpdccopy
grow 85
shrink 85
shrink 85
grow 85
or mvpdccopy
templayer pccopy CONT
and polycheck
layer pc pccopy
grow 85
shrink 85
shrink 85
grow 85
or pccopy
templayer nsccopy CONT
and nsubcheck
layer nsc nsccopy
grow 85
shrink 85
shrink 85
grow 85
or nsccopy
templayer mvnsccopy CONT
and mvnsubcheck
layer mvnsc mvnsccopy
grow 85
shrink 85
shrink 85
grow 85
or mvnsccopy
templayer psccopy CONT
and psubcheck
layer psc psccopy
grow 85
shrink 85
shrink 85
grow 85
or psccopy
templayer mvpsccopy CONT
and mvpsubcheck
layer mvpsc mvpsccopy
grow 85
shrink 85
shrink 85
grow 85
or mvpsccopy
templayer barelicont CONT
and LI
and-not DIFF,TAP
and-not POLY
and-not DIODE
and-not nsubcheck
and-not psubcheck
and-not mvnsubcheck
and-not mvpsubcheck
and-not CORELI
copyup barelicont
templayer barecont CONT
and-not LI
and-not nsubcheck
and-not psubcheck
and-not mvnsubcheck
and-not mvpsubcheck
and-not CORELI
copyup barecont
layer glass GLASS,PADTXT,PADPIN
templayer boundary BOUND,STDCELL,PADCELL
layer comment LVSTEXT
layer comment TTEXT
templayer obspoly FILLOBSPOLY
and-not POLY
layer obsactive FILLOBSFOM
and-not DIFF,TAP
or obspoly
layer obsm1 FILLOBSM1
and-not MET1
layer obsm2 FILLOBSM2
and-not MET2
layer obsm3 FILLOBSM3
and-not MET3
layer obsm4 FILLOBSM4
and-not MET4
layer obsm5 FILLOBSM5
and-not MET5
layer var POLY
and TAP
and NSDM
and NWELL,nwelcheck
and-not HVI,hvcheck
and-not HVTP
and-not COREID
layer varhvt POLY
and TAP
and NSDM
and NWELL,nwelcheck
and-not HVI,hvcheck
and HVTP
layer mvvar POLY
and TAP
and NSDM
and NWELL,nwelcheck
and HVI,hvcheck
calma NWELL 64 20
calma DIFF 65 20
calma TAP  65 44
calma DNWELL 64 18
calma SUBCUT 81 53
calma PWRES 64 13
calma LVTN 125 44
calma HVTR 18 20
calma HVTP 78 44
calma SONOS 80 20
calma NSDM 93 44
calma PSDM 94 20
calma HVI 75 20
calma EDID 81 57
calma NPC 95 20
calma RPM 86 20
calma URPM 79 20
calma LDNTM 11 44
calma HVNTM 125 20
calma POLYRES 66 13
calma DIFFRES 65 13
calma POLY 66 20
calma POLYMOD 66 83
calma LVID 81 60
calma DIODE 81 23
calma NPNID 82 20
calma PNPID 82 44
calma CAPID 82 64
calma COREID 81 2
calma PHOTO 81 81
calma STDCELL 81 4
calma PADCELL 81 3
calma SEALID 81 1
calma LOWTAPDENSITY 81 14
calma ESDID 81 19
calma OUTLINE 236 0
calma POLYCUT 66 14
calma POLYGATE 66 9
calma DIFFCUT 65 14
calma HVNWELLID 81 63
calma MET5BLOCK 72 10
calma PADDIFFID 81 6
calma PADMETALID 81 8
calma PADCENTERID 81 20
calma CONT 66 44
calma LI   67 20
calma MCON 67 44
calma MET1 68 20
calma VIA1 68 44
calma MET2 69 20
calma VIA2 69 44
calma MET3 70 20
calma VIA3 70 44
calma MET4 71 20
calma VIA4 71 44
calma MET5 72 20
calma RDL 74 20
calma GLASS 76 20
calma SUBTXT  64 59
calma PADTXT  76 5
calma DIFFTXT 65 6
calma TAPTXT  65 5
calma WELLTXT 64 5
calma LITXT   67 5
calma POLYTXT 66 5
calma MET1TXT 68 5
calma MET2TXT 69 5
calma MET3TXT 70 5
calma MET4TXT 71 5
calma MET5TXT 72 5
calma RDLTXT 74 5
calma LIRES 67 13
calma MET1RES 68 13
calma MET2RES 69 13
calma MET3RES 70 13
calma MET4RES 71 13
calma MET5RES 72 13
calma LIFILL   56 28
calma MET1FILL 36 28
calma MET2FILL 41 28
calma MET3FILL 34 28
calma MET4FILL 51 28
calma MET5FILL 59 28
calma POLYSHORT 66 15
calma LISHORT 67 15
calma MET1SHORT 68 15
calma MET2SHORT 69 15
calma MET3SHORT 70 15
calma MET4SHORT 71 15
calma MET5SHORT 72 15
calma SUBPIN 122 16
calma PADPIN 76 16
calma DIFFPIN 65 16
calma POLYPIN 66 16
calma WELLPIN 64 16
calma LIPIN 67 16
calma MET1PIN 68 16
calma MET2PIN 69 16
calma MET3PIN 70 16
calma MET4PIN 71 16
calma MET5PIN 72 16
calma RDLPIN 74 16
calma BOUND 235 4
calma LVSTEXT 83 44
calma CAPM 89 44
calma CAPM2 97 44
calma FILLOBSM1  62  24
calma FILLOBSM2  105 52
calma FILLOBSM3  107 24
calma FILLOBSM4  112  4
calma FILLOBSM5  117  4
calma FILLOBSFOM  22 24
calma FILLOBSPOLY 33 24
calma FOMFILL	  23  28
calma POLYFILL	  28  28
calma LIFILL	  56  28
calma MET1FILL	  36  28
calma MET2FILL	  41  28
calma MET3FILL	  34  28
calma MET4FILL	  51  28
calma MET5FILL	  59  28
end
extract
style riku
device msubcircuit sky130_fd_pr__pfet_01v8 pfet,scpfet *pdiff,pdiffres *pdiff,pdiffres nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__special_pfet_latch ppu *pdiff,pdiffres *pdiff,pdiffres nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__pfet_01v8_lvt pfetlvt *pdiff,pdiffres *pdiff,pdiffres nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__pfet_01v8_mvt pfetmvt *pdiff,pdiffres *pdiff,pdiffres nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__pfet_01v8_hvt pfethvt,scpfethvt *pdiff,pdiffres *pdiff,pdiffres nwell error w>=0.42 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__special_pfet_01v8_hvt scpfethvt *pdiff,pdiffres *pdiff,pdiffres nwell error w<0.42 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__nfet_01v8 nfet,scnfet *ndiff,ndiffres *ndiff,ndiffres pwell,space/w error w>=0.42 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__special_nfet_01v8 scnfet *ndiff,ndiffres *ndiff,ndiffres pwell,space/w error w<0.42 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__special_nfet_latch npd *ndiff,ndiffres *ndiff,ndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__special_nfet_latch npd *ndiff,ndiffres *srampvar pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__special_nfet_pass npass *ndiff,ndiffres *ndiff,ndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__nfet_01v8_lvt nfetlvt *ndiff,ndiffres *ndiff,ndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_bs_flash__special_sonosfet_star nsonos *ndiff,ndiffres *ndiff,ndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__npn_05v5 npn *ndiff dnwell space/w error a1=area
device msubcircuit sky130_fd_pr__npn_05v5_W1p00L1p00 npn *ndiff dnwell space/w error a1>0.99 a1<1.01
device msubcircuit sky130_fd_pr__npn_05v5_W1p00L2p00 npn *ndiff dnwell space/w error a1>1.99 a1<2.01
device msubcircuit sky130_fd_pr__pnp_05v5 pnp *pdiff pwell,space/w a1=area
device msubcircuit sky130_fd_pr__pnp_05v5_W0p68L0p68 pnp *pdiff pwell,space/w a1>0.45 a1<0.47
device msubcircuit sky130_fd_pr__pnp_05v5_W3p40L3p40 pnp *pdiff pwell,space/w a1>11.55 a1<11.57
device msubcircuit sky130_fd_pr__npn_11v0 npn *mvndiff dnwell space/w error a1=area
device msubcircuit sky130_fd_pr__npn_11v0_W1p00L1p00 npn *mvndiff dnwell space/w error a1>0.99 a1<1.01
device msubcircuit Ignore mvnfet *mvndiff,mvndiffres dnwell pwell,space/w error +npn,pnp
device msubcircuit Ignore mvpfet *mvpdiff,mvpdiffres pwell,space/w nwell error +npn,pnp
device msubcircuit sky130_fd_pr__nfet_g5v0d16v0 mvnfet *mvndiff extdrain,*mvnsd pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__pfet_g5v0d16v0 mvpfet *mvpdiff extdrain,*mvpsd nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__nfet_20v0_nvt mvnnfet *mvndiff,mvndiffres dnwell pwell,space/w error l=l w=w a1=as a2=ad p1=ps p2=pd
device msubcircuit sky130_fd_pr__nfet_20v0 mvnfet *mvndiff,mvndiffres dnwell pwell,space/w error l=l w=w a1=as a2=ad p1=ps p2=pd
device msubcircuit sky130_fd_pr__pfet_20v0 mvpfet *mvpdiff,mvpdiffres pwell,space/w nwell error l=l w=w a1=as a2=ad p1=ps p2=pd
device msubcircuit sky130_fd_pr__pfet_g5v0d10v5 mvpfet *mvpdiff,mvpdiffres *mvpdiff,mvpdiffres nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__nfet_g5v0d10v5 mvnfet *mvndiff,mvndiffres *mvndiff,mvndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__nfet_05v0_nvt mvnnfet *mvndiff,mvndiffres *mvndiff,mvndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__nfet_03v3_nvt nnfet *mvndiff,mvndiffres *mvndiff,mvndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__esd_nfet_g5v0d10v5 mvnfetesd *mvndiff,mvndiffres *mvndiff,mvndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__esd_pfet_g5v0d10v5 mvpfetesd *mvpdiff,mvpdiffres *mvpdiff,mvpdiffres nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sky130_fd_pr__diode_pw2nd_05v5 *ndiode pwell,space/w a=area*1E12 p=perim*1E6
device msubcircuit sky130_fd_pr__diode_pw2nd_05v5_lvt *ndiodelvt pwell,space/w a=area*1E12 p=perim*1E6
device msubcircuit sky130_fd_pr__diode_pw2nd_05v5_nvt *nndiode pwell,space/w a=area*1E12 p=perim*1E6
device msubcircuit sky130_fd_pr__diode_pw2nd_11v0 *mvndiode pwell,space/w a=area*1E12 p=perim*1E6
device mosfet sky130_fd_pr__pfet_01v8 scpfet,pfet pdiff,pdiffres,pdc nwell
device mosfet sky130_fd_pr__special_pfet_latch ppu pdiff,pdiffres,pdc nwell
device mosfet sky130_fd_pr__pfet_01v8_lvt pfetlvt pdiff,pdiffres,pdc nwell
device mosfet sky130_fd_pr__pfet_01v8_mvt pfetmvt pdiff,pdiffres,pdc nwell
device mosfet sky130_fd_pr__pfet_01v8_hvt scpfethvt,pfethvt pdiff,pdiffres,pdc nwell
device mosfet sky130_fd_pr__nfet_01v8 scnfet,nfet ndiff,ndiffres,ndc pwell,space/w
device mosfet sky130_fd_pr__special_nfet_pass npass ndiff,ndiffres,ndc pwell,space/w
device mosfet sky130_fd_pr__special_nfet_latch npd ndiff,ndiffres,ndc pwell,space/w
device mosfet sky130_fd_pr__nfet_01v8_lvt nfetlvt ndiff,ndiffres,ndc pwell,space/w
device mosfet sky130_fd_bs_flash__special_sonosfet_star nsonos ndiff,ndiffres,ndc pwell,space/w
device mosfet sky130_fd_pr__nfet_20v0_nvt mvnnfet *mvndiff,mvndiffres dnwell pwell,space/w error
device mosfet sky130_fd_pr__nfet_20v0 mvnfet *mvndiff,mvndiffres dnwell pwell,space/w error
device mosfet sky130_fd_pr__pfet_20v0 mvpfet *mvpdiff,mvpdiffres pwell,space/w nwell error
device mosfet sky130_fd_pr__pfet_g5v0d10v5 mvpfet mvpdiff,mvpdiffres,mvpdc nwell
device mosfet sky130_fd_pr__esd_pfet_g5v0d10v5 mvpfetesd mvpdiff,mvpdiffres,mvpdc nwell
device mosfet sky130_fd_pr__nfet_g5v0d10v5 mvnfet mvndiff,mvndiffres,mvndc pwell,space/w
device mosfet sky130_fd_pr__esd_nfet_g5v0d10v5 mvnfetesd mvndiff,mvndiffres,mvndc pwell,space/w
device mosfet sky130_fd_pr__nfet_05v0_nvt mvnnfet *mvndiff,mvndiffres pwell,space/w
device mosfet sky130_fd_pr__nfet_03v3_nvt nnfet *mvndiff,mvndiffres pwell,space/w
end
"#;

/// `gf180mcuD/libs.tech/magic/gf180mcuD.tech`: 23 líneas `device` de MOS.
pub const GF180: &str = r#"types
dwell deepnwell,dnwell,dnw
dwell isosubstrate,isosub
well nwell,nw
well pwell,pw
well rnw,rnwell
well pbase,npn
well nbase,pnp
active nmos,ntransistor,nfet
active pmos,ptransistor,pfet
active nnmos,nntransistor,nnfet
active mvnmos,mvntransistor,mvnfet
active mvpmos,mvptransistor,mvpfet
active mvnnmos,mvnntransistor,mvnnfet
active ndiff,ndiffusion,ndif
active pdiff,pdiffusion,pdif
active mvndiff,mvndiffusion,mvndif
active mvpdiff,mvpdiffusion,mvpdif
active ndiffc,ndcontact,ndc
active pdiffc,pdcontact,pdc
active mvndiffc,mvndcontact,mvndc
active mvpdiffc,mvpdcontact,mvpdc
active psubdiff,psubstratepdiff,ppdiff,ppd,psd
active nsubdiff,nsubstratendiff,nndiff,nnd,nsd
active mvpsubdiff,mvpsubstratepdiff,mvppdiff,mvppd,mvpsd
active mvnsubdiff,mvnsubstratendiff,mvnndiff,mvnnd,mvnsd
active psubdiffcont,psubstratepcontact,psc
active nsubdiffcont,nsubstratencontact,nsc
active mvpsubdiffcont,mvpsubstratepcontact,mvpsc
active mvnsubdiffcont,mvnsubstratencontact,mvnsc
active ldndiff,ldndiffusion,ldndif
active ldpdiff,ldpdiffusion,ldpdif
active ldndiffc,ldndcontact,ldndc
active ldpdiffc,ldpdcontact,ldpdc
active nvaractor,nvaract,nvar
active pvaractor,pvaract,pvar
active mvnvaractor,mvnvaract,mvnvar
active mvpvaractor,mvpvaract,mvpvar
-active nmoscap,ncap
-active pmoscap,pcap
-active mvnmoscap,mvncap
-active mvpmoscap,mvpcap
active polysilicon,poly,p
active polycontact,pcontact,polycut,pc,polyc
active npolyres,npres,rnp
active ppolyres,ppres,rpp
active npolysilicide,nsresistor,nspres,rnps
active ppolysilicide,psresistor,pspres,rpps
active nhighres,nhires,hires
active mvnhighres,mvnhires,mvhires
active ndiffres,rnd,rdn,rndiff
active pdiffres,rpd,rdp,rpdiff
active ndiffsilicide,rnds,rdns,rndiffs
active pdiffsilicide,rpds,rdps,rpdiffs
active mvndiffres,mvrnd,mvrdn,mvrndiff
active mvpdiffres,mvrpd,mvrdp,mvrpdiff
active mvndiffsilicide,mvrnds,mvrdns,mvrndiffs
active mvpdiffsilicide,mvrpds,mvrdps,mvrpdiffs
active schottky,skdi
active schottkyc,skdic
active pdiode,pdi
active ndiode,ndi
active nndiode,nndi
active pdiodec,pdic
active ndiodec,ndic
active nndiodec,nndic
active mvpdiode,mvpdi
active mvndiode,mvndi
active mvnndiode,mvnndi
active mvpdiodec,mvpdic
active mvndiodec,mvndic
active mvnndiodec,mvnndic
metal1 metal1,m1,met1
metal1 rmetal1,rm1,rmet1
metal1 via1,m2contact,m2cut,m2c,via,v,v1
metal2 metal2,m2,met2
metal2 rmetal2,rm2,rmet2
metal2 via2,m3contact,m3cut,m3c,v2
metal4 mimcap,mim,capm
metal4 mimcapcontact,mimcapc,mimcc,capmc
metal3 metal3,m3,met3
metal3 rmetal3,rm3,rmet3
metal3 via3,v3
metal4 metal4,m4,met4
metal4 rmetal4,rm4,rmet4
metal4 via4,v4
metal5 metal5,m5,met5
metal5 rm5,rmetal5,rmet5
end
cifinput
style riku
layer pwell PWELL,PWELLTXT
and-not BJTDEF,BJTDRC
layer pbase PWELL,PWELLTXT
and BJTDEF,BJTDRC
layer nwell NWELL,NWELLTXT
and-not BJTDEF,BJTDRC
layer nbase NWELL,NWELLTXT
and BJTDEF,BJTDRC
layer dnwell DNWELL
layer isosub SUBCUT
templayer nwelldef DNWELL
shrink 500
and-not PWELL
or NWELL
templayer ndiffarea DIFF
and-not POLY
and-not nwelldef
and-not PPLUS
and-not SBLK
and-not DUALGATE
and NPLUS
copyup ndifcheck
layer ndiff ndiffarea
layer filldiff DIFFFILL
templayer xndifcheck ndifcheck
copyup ndifcheck
templayer mvndiffarea DIFF
and-not POLY
and-not nwelldef
and-not PPLUS
and-not SBLK
and DUALGATE
and NPLUS
copyup mvndifcheck
layer mvndiff mvndiffarea
templayer mvxndifcheck mvndifcheck
copyup mvndifcheck
templayer sccathode SCHOTTKY
and DIFF
and NPLUS
layer schottky SCHOTTKY
and DIFF
and-not NPLUS
and-not PPLUS
or sccathode
grow 140
shrink 140
and-not sccathode
layer schottkyc SCHOTTKY
and CONT
and DIFF
and-not NPLUS
and-not PPLUS
grow 145
shrink 140
layer ndiode DIFF
and NPLUS
and DIODE
and-not nwelldef
and-not POLY
and-not PPLUS
and-not DUALGATE
and-not NAT
layer nndiode DIFF
and NPLUS
and DIODE
and-not nwelldef
and-not POLY
and-not PPLUS
and-not DUALGATE
and NAT
templayer ndiodearea DIODE
and NPLUS
and-not nwelldef
and-not DUALGATE
copyup DIODE,NPLUS
layer ndiffres DIFF
and-not POLY
and SBLK
and NPLUS
and-not DUALGATE
templayer pdiffarea DIFF
and-not POLY
and nwelldef
and-not NPLUS
and-not SBLK
and-not DIODE
and PPLUS
and-not DUALGATE
copyup pdifcheck
layer pdiff pdiffarea
layer mvndiode DIFF
and NPLUS
and DIODE
and-not POLY
and-not PPLUS
and DUALGATE
and-not NAT
layer mvnndiode DIFF
and NPLUS
and DIODE
and-not POLY
and-not PPLUS
and DUALGATE
and NAT
templayer mvndiodearea DIODE
and NPLUS
and-not nwelldef
and DUALGATE
copyup DIODE,NPLUS
layer mvndiffres DIFF
and-not POLY
and SBLK
and NPLUS
and DUALGATE
templayer mvpdiffarea DIFF
and-not POLY
and nwelldef
and-not NPLUS
and-not SBLK
and-not DIODE
and DUALGATE
and PPLUS
copyup mvpdifcheck
layer mvpdiff mvpdiffarea
templayer xpdifcheck pdifcheck
copyup pdifcheck
layer pdiode DIFF
and PPLUS
and-not POLY
and-not NPLUS
and-not DUALGATE
and DIODE
templayer pdiodearea DIODE
and PPLUS
copyup DIODE,PPLUS
templayer pfetarea DIFF
and-not NPLUS
and-not DUALGATE
and POLY
layer pfet pfetarea
and-not MOSCAP
layer pcap pfetarea
and MOSCAP
templayer pfetexpand pfetarea
grow 530
layer nwell pfetarea
grow 310
templayer mvxpdifcheck mvpdifcheck
copyup mvpdifcheck
layer mvpdiode DIFF
and PPLUS
and-not POLY
and-not NPLUS
and-not RESDEF
and DUALGATE
and DIODE
templayer mvpdiodearea DIODE
and PPLUS
copyup DIODE,PPLUS
templayer mvpfetarea DIFF
and DUALGATE
and-not NPLUS
and POLY
layer mvpfet mvpfetarea
and-not MOSCAP
layer mvpcap mvpfetarea
and MOSCAP
templayer mvpfetexpand mvpfetarea
grow 530
layer pdiff DIFF
and-not DUALGATE
and-not NPLUS
and-not POLY
and nwelldef
and pfetexpand
layer pdiffres DIFF
and-not POLY
and PPLUS
and nwelldef
and SBLK
layer nfet DIFF
and POLY
and-not PPLUS
and-not DUALGATE
and-not nwelldef
and NPLUS
and-not NAT
and-not MOSCAP
layer ncap DIFF
and POLY
and-not PPLUS
and-not DUALGATE
and-not nwelldef
and NPLUS
and-not NAT
and MOSCAP
layer nnfet DIFF
and POLY
and-not PPLUS
and-not DUALGATE
and-not nwelldef
and NPLUS
and NAT
templayer nsdarea DIFF
and NPLUS
and nwelldef
and-not POLY
and-not PPLUS
and-not DUALGATE
layer nsd nsdarea
templayer nsdexpand nsdarea
grow 500
templayer xnsubcheck nsubcheck
copyup nsubcheck
templayer psdarea DIFF
and PPLUS
and-not DUALGATE
and-not nwelldef
and-not POLY
and-not NPLUS
and-not pfetexpand
copyup psubcheck
layer psd psdarea
templayer psdexpand psdarea
grow 500
layer mvpdiff DIFF
and-not NPLUS
and-not POLY
and nwelldef
and DUALGATE
and mvpfetexpand
layer mvpdiffres DIFF
and-not POLY
and PPLUS
and SBLK
and DUALGATE
layer mvnfet DIFF
and POLY
and-not PPLUS
and NPLUS
and-not NAT
and-not nwelldef
and DUALGATE
and-not MOSCAP
layer mvncap DIFF
and POLY
and-not PPLUS
and NPLUS
and-not NAT
and-not nwelldef
and DUALGATE
and MOSCAP
layer mvnnfet DIFF
and POLY
and-not PPLUS
and NPLUS
and NAT
and-not nwelldef
and DUALGATE
templayer mvnsdarea DIFF
and NPLUS
and-not POLY
and-not PPLUS
and nwelldef
and DUALGATE
copyup mvnsubcheck
layer mvnsd mvnsdarea
templayer mvnsdexpand mvnsdarea
grow 500
templayer mvxnsubcheck mvnsubcheck
copyup mvnsubcheck
templayer mvpsdarea DIFF
and PPLUS
and-not nwelldef
and-not POLY
and-not NPLUS
and DUALGATE
and-not mvpfetexpand
copyup mvpsubcheck
layer mvpsd mvpsdarea
templayer mvpsdexpand mvpsdarea
grow 500
templayer xpsubcheck psubcheck
copyup psubcheck
templayer mvxpsubcheck mvpsubcheck
copyup mvpsubcheck
layer psd DIFF
and-not PPLUS
and-not NPLUS
and-not POLY
and-not DUALGATE
and-not pfetexpand
and psdexpand
layer nsd DIFF
and-not PPLUS
and-not NPLUS
and-not POLY
and nwelldef
and-not DUALGATE
and nsdexpand
layer mvpsd DIFF
and-not PPLUS
and-not NPLUS
and-not POLY
and-not nwelldef
and DUALGATE
and-not mvpfetexpand
and mvpsdexpand
layer mvnsd DIFF
and-not PPLUS
and-not NPLUS
and-not POLY
and nwelldef
and DUALGATE
and mvnsdexpand
templayer polyarea POLY
and-not DIFF
and-not SBLK
and-not PLFUSE
and-not HRES
copyup polycheck
layer poly polyarea,POLYTXT
and-not RESDEF
layer fillpoly POLYFILL
templayer xpolycheck polycheck
copyup polycheck
layer rpps POLY
and-not SBLK
and PPLUS
and RESDEF
layer rnps POLY
and-not SBLK
and NPLUS
and RESDEF
layer rpp POLY
and SBLK
and PPLUS
and-not HRES
and RESDEF
layer poly POLY
and-not DIFF
and SBLK
and-not RESDEF
layer efuse POLY
and-not DIFF
and PLFUSE
layer rnp POLY
and SBLK
and NPLUS
and RESDEF
and-not HRES
layer hires POLY
and SBLK
and HRES
and RESDEF
and-not DUALGATE
layer mvhires POLY
and SBLK
and HRES
and RESDEF
and DUALGATE
layer poly POLY
and HRES
and-not SBLK
and-not RESDEF
layer ndc CONT
and DIFF
and NPLUS
and-not nwelldef
and MET1
and-not DUALGATE
and-not DIODE
grow 145
shrink 140
layer nsc CONT
and DIFF
and NPLUS
and nwelldef
and MET1
and-not DUALGATE
and-not DIODE
grow 145
shrink 140
layer pdc CONT
and DIFF
and PPLUS
and nwelldef
and MET1
and-not DUALGATE
and-not DIODE
grow 145
shrink 140
layer pdc CONT
and DIFF
and PPLUS
and MET1
and-not DUALGATE
and-not DIODE
and pfetexpand
grow 145
shrink 140
layer psc CONT
and DIFF
and PPLUS
and-not nwelldef
and MET1
and-not DUALGATE
and-not DIODE
and-not pfetexpand
grow 145
shrink 140
layer pc CONT
and POLY
and-not DIFF
and MET1
grow 145
shrink 140
layer ndic CONT
and DIFF
and NPLUS
and DIODE
and-not POLY
and-not PPLUS
and-not DUALGATE
and-not NAT
grow 145
shrink 140
layer nndic CONT
and DIFF
and NPLUS
and DIODE
and-not POLY
and-not PPLUS
and-not DUALGATE
and NAT
grow 145
shrink 140
layer pdic CONT
and DIFF
and PPLUS
and DIODE
and-not POLY
and-not NPLUS
and-not DUALGATE
grow 145
shrink 140
layer mvndc CONT
and DIFF
and NPLUS
and-not nwelldef
and MET1
and DUALGATE
and-not DIODE
grow 145
shrink 140
layer mvnsc CONT
and DIFF
and NPLUS
and MET1
and DUALGATE
and nwelldef
and-not DIODE
grow 145
shrink 140
layer mvpdc CONT
and DIFF
and PPLUS
and MET1
and DUALGATE
and nwelldef
and-not DIODE
grow 145
shrink 140
layer mvpdc CONT
and DIFF
and PPLUS
and MET1
and DUALGATE
and-not DIODE
and mvpfetexpand
grow 145
shrink 140
layer mvpsc CONT
and DIFF
and PPLUS
and-not nwelldef
and MET1
and DUALGATE
and-not DIODE
and-not mvpfetexpand
grow 145
shrink 140
layer mvndic CONT
and DIFF
and NPLUS
and DIODE
and-not POLY
and-not PPLUS
and DUALGATE
and-not NAT
grow 145
shrink 140
layer mvnndic CONT
and DIFF
and NPLUS
and DIODE
and-not POLY
and-not PPLUS
and DUALGATE
and NAT
grow 145
shrink 140
layer mvpdic CONT
and DIFF
and PPLUS
and DIODE
and-not POLY
and-not NPLUS
and DUALGATE
grow 145
shrink 140
layer rm1 MET1
and RESDEF
and MET1RES
layer m1 MET1,MET1TXT
and-not MET1RES
layer obsm1 M1BLOCK
layer fillm1 M1FILL
layer m2c VIA1
grow 130
shrink 130
layer rm2 MET2
and RESDEF
and MET2RES
layer m2 MET2,MET2TXT
and-not MET2RES
layer obsm2 M2BLOCK
layer fillm2 M2FILL
layer rm3 MET3
and RESDEF
and MET3RES
layer m3 MET3,MET3TXT
and-not MET3RES
layer obsm3 M3BLOCK
layer fillm3 M3FILL
layer m3c VIA2
grow 140
shrink 130
layer rm4 MET4
and RESDEF
and MET4RES
layer m4 MET4,MET4TXT
and-not MET4RES
layer obsm4 M4BLOCK
layer fillm4 M4FILL
layer via3 VIA3
grow 140
shrink 130
layer rm5 MET5
and RESDEF
and MET5RES
templayer mimarea CAPDEF
and MET4
layer m5 MET5,MET5TXT
and-not MET5RES
layer obsm5 M5BLOCK
layer fillm5 M5FILL
layer via4 VIA4
and-not CAPM
and-not mimarea
grow 140
shrink 130
layer mimcc VIA4
and MET5
and CAPM
and CAPDEF
grow 260
shrink 250
layer mimcap CAPM
and CAPDEF
templayer nolayer CAP_LENGTH
templayer gentrans DIFF
and-not PPLUS
and-not NPLUS
and POLY
copyup DIFF,POLY
templayer gendiff DIFF
and-not PPLUS
and-not NPLUS
and-not POLY
copyup DIFF
layer ndic CONT
and MET1
and DIODE
and NPLUS
and-not DUALGATE
and-not NAT
grow 100
shrink 100
layer mvndic CONT
and MET1
and DIODE
and NPLUS
and DUALGATE
and-not NAT
grow 100
shrink 100
layer mvnndic CONT
and MET1
and DIODE
and NPLUS
and DUALGATE
and NAT
grow 100
shrink 100
layer pdic CONT
and MET1
and DIODE
and PPLUS
and-not DUALGATE
grow 100
shrink 100
layer mvpdic CONT
and MET1
and DIODE
and PPLUS
and DUALGATE
grow 100
shrink 100
layer ndc CONT
and ndifcheck
grow 100
shrink 100
layer mvndc CONT
and mvndifcheck
grow 100
shrink 100
layer pdc CONT
and pdifcheck
grow 100
shrink 100
layer mvpdc CONT
and mvpdifcheck
grow 100
shrink 100
layer pc CONT
and polycheck
grow 100
shrink 100
layer nsc CONT
and nsubcheck
grow 100
shrink 100
layer mvnsc CONT
and mvnsubcheck
grow 100
shrink 100
layer psc CONT
and psubcheck
grow 100
shrink 100
layer mvpsc CONT
and mvpsubcheck
grow 100
shrink 100
templayer gencont CONT
and MET1
and-not DIFF
and-not POLY
and-not DIODE
and-not nsubcheck
and-not psubcheck
and-not mvnsubcheck
and-not mvpsubcheck
copyup CONT,MET1
templayer barecont CONT
and-not MET1
and-not nsubcheck
and-not psubcheck
and-not mvnsubcheck
and-not mvpsubcheck
copyup CONT
layer glass GLASS
templayer cellbound BOUND,PRBOUND
layer lvstext TTEXT
layer fillblock  FILLOBS2
layer nvar POLY
and DIFF
and NPLUS
and nwelldef
and-not DUALGATE
layer mvnvar POLY
and DIFF
and NPLUS
and nwelldef
and DUALGATE
layer pvar POLY
and DIFF
and PPLUS
and-not nwelldef
and-not DUALGATE
layer mvpvar POLY
and DIFF
and PPLUS
and-not nwelldef
and DUALGATE
calma DNWELL 12 0
calma NWELL 21 0
calma NWELLTXT 21 10
calma PWELL 204 0
calma PWELLTXT 204 10
calma SUBCUT 23 5
calma DIFF 22 0
calma DIFFFILL 22 4
calma POLY 30 0
calma POLYFILL 30 4
calma POLYTXT 30 10
calma NPLUS 32 0
calma PPLUS 31 0
calma SBLK 49 0
calma GLASS 37 0
calma CONT 33 0
calma MET1 34 0
calma MET1TXT 34 10
calma M1BLOCK 34 5
calma M1FILL 34 4
calma MET2RES 110 11
calma VIA1 35 0
calma MET2 36 0
calma MET2TXT 36 10
calma M2BLOCK 36 5
calma M2FILL 36 4
calma MET2RES 110 12
calma VIA2 38 0
calma MET3 42 0
calma MET3TXT 42 10
calma M3BLOCK 42 5
calma M3FILL 42 4
calma MET3RES 110 13
calma VIA3 40 0
calma MET4 46 0
calma MET4TXT 46 10
calma M4BLOCK 46 5
calma M4FILL 46 4
calma MET4RES 110 14
calma VIA4 41 0
calma MET5 81 0
calma MET5TXT 81 10
calma M5BLOCK 81 5
calma M5FILL 81 4
calma MET5RES 110 15
calma HRES 62 0
calma EFUSE 80 5
calma PLFUSE 125 5
calma SOURCE 100 8
calma NAT 5 0
calma CAPM 75 0
calma CAP_LENGTH 117 10
calma DIODE  115 5
calma SCHOTTKY 241 0
calma CAPDEF 117 5
calma BJTDEF 118 5
calma BJTDRC 127 5
calma MOSCAP 166 5
calma BOUND 0 0
calma PRBOUND 63 0
calma VTEXT 63 63
calma FILLOBS  111 5
calma FILLOBS2 152 5
calma TTEXT    230 *
calma RESDEF   110 *
calma DUALGATE 55 0
calma SRAMDEF  108 5
calma FET5VDEF 112 1
end
extract
style riku
device msubcircuit pfet_03v3 pfet pdiff,pdc pdiff,pdc allnwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit nfet_03v3 nfet ndiff,ndc ndiff,ndc allpsub error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit pfet_06v0 mvpfet mvpdiff,mvpdc mvpdiff,mvpdc allnwell error l>=5.5e-7 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit pfet_05v0 mvpfet mvpdiff,mvpdc mvpdiff,mvpdc allnwell error l<5.5e-7 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit nfet_06v0 mvnfet mvndiff,mvndc mvndiff,mvndc allpsub error l>=7e-7 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit nfet_05v0 mvnfet mvndiff,mvndc mvndiff,mvndc allpsub error l<7e-7 l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit pfet_03v3_dss pfet pdiffres pdiffres allnwell error l=l w=w a1=as p1=ps a2=ad p2=pd l1=s_sab l2=d_sab
device msubcircuit nfet_03v3_dss nfet ndiffres ndiffres allpsub error l=l w=w a1=as p1=ps a2=ad p2=pd l1=s_sab l2=d_sab
device msubcircuit pfet_06v0_dss mvpfet mvpdiffres mvpdiffres allnwell error l=l w=w a1=as p1=ps a2=ad p2=pd l1=s_sab l2=d_sab
device msubcircuit nfet_06v0_dss mvnfet mvndiffres mvndiffres allpsub error l=l w=w a1=as p1=ps a2=ad p2=pd l1=s_sab l2=d_sab
device msubcircuit nfet_06v0_nvt mvnnfet mvndiff,mvndiffres,mvndc mvndiff,mvndiffres,mvndc allpsub error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit npn_10p00x10p00 npn *ndiff dnwell space/w error a1>99.0 a1<101.0
device msubcircuit npn_05p00x05p00 npn *ndiff dnwell space/w error a1>24.0 a1<26.0
device msubcircuit npn_00p54x16p00 npn *ndiff dnwell space/w error a1>8.5 a1<8.7
device msubcircuit npn_00p54x08p00 npn *ndiff dnwell space/w error a1>4.2 a1<4.4
device msubcircuit npn_00p54x04p00 npn *ndiff dnwell space/w error a1>2.0 a1<2.2
device msubcircuit npn_00p54x02p00 npn *ndiff dnwell space/w error a1>1.0 a1<1.2
device msubcircuit pnp_10p00x00p42 pnp *pdiff pwell,space/w error a1>4.1 a1<4.3
device msubcircuit pnp_05p00x00p42 pnp *pdiff pwell,space/w error a1>2.0 a1<2.2
device msubcircuit pnp_10p00x10p00 pnp *pdiff pwell,space/w error a1>99.0 a1<101.0
device msubcircuit pnp_05p00x05p00 pnp *pdiff pwell,space/w error a1>24.0 a1<26.0
device msubcircuit nfet_10v0_asym mvnfet *mvndiff *ldndiff allpsub error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit pfet_10v0_asym mvpfet *mvpdiff *ldpdiff allnwell error l=l w=w a1=as p1=ps a2=ad p2=pd
end
"#;

/// `ihp-sg13g2/libs.tech/magic/ihp-sg13g2.tech`: 14 líneas `device` de MOS.
pub const IHP: &str = r#"types
dwell dnwell,dnw
dwell isosubstrate,isosub
well nwell,nw
well pwell,pw
well pbase,npn
well nbase,pnp
active nmos,ntransistor,nfet
active pmos,ptransistor,pfet
active hvnmos,hvntransistor,hvnfet
active hvpmos,hvptransistor,hvpfet
-active hvnmosesd,hvntransistoresd,hvnfetesd
-active hvpmosesd,hvptransistoresd,hvpfetesd
active hvvaractor,hvvaract,hvvar
active hvvarcontact,hvvarc,hvvc
active hvpvaractor,hvpvaract,hvpvar
-active sealcont,sealc
active ndiff,ndiffusion,ndif
active pdiff,pdiffusion,pdif
active pbasec,pbcontact,pbc
active nemitter,nemit,ne
active nemitterc,nemitc,necontact,nec
active gemitterc,gemitc,gecontact,gec
active hvnemitter,hvnemit,hvne
active hvnemitterc,hvnemitc,hvnecontact,hvnec
active hvndiff,hvndiffusion,hvndif
active hvpdiff,hvpdiffusion,hvpdif
active ndiffc,ndcontact,ndc
active pdiffc,pdcontact,pdc
active hvndiffc,hvndcontact,hvndc
active hvpdiffc,hvpdcontact,hvpdc
active psubdiff,psubstratepdiff,ppdiff,ppd,psd,ptap
active nsubdiff,nsubstratendiff,nndiff,nnd,nsd,ntap
active hvpsubdiff,hvpsubstratepdiff,hvppdiff,hvppd,hvpsd,hvptap
active hvnsubdiff,hvnsubstratendiff,hvnndiff,hvnnd,hvnsd,hvntap
active psubdiffcont,psubstratepcontact,psc,ptapc
active nsubdiffcont,nsubstratencontact,nsc,ntapc
active hvpsubdiffcont,hvpsubstratepcontact,hvpsc,hvptapc
active hvnsubdiffcont,hvnsubstratencontact,hvnsc,hvntapc
active poly,p,polysilicon
active polycont,pc,pcontact,polycut,polyc
active npolyres,nres,rsil
active ppolyres,pres,rppd
active xpolyres,xres,rhigh
active hvndiffres,hvrnd,hvrndiff
active isodiffres,risodiff,riso
active hvisodiffres,hvrisodiff,hvriso
active pdiode,pdi
active ndiode,ndi
active pdiodecont,pdiodec,pdic
active ndiodecont,ndiodec,ndic
active schottky,sdi
active schottkycont,schottkyc,sdic
metal1 metal1,m1,met1
metal1 rmetal1,rm1,rmet1
metal1 via1,m2contact,m2cut,m2c,via,v,v1
-metal1 sealvia1,sealv1
metal2 metal2,m2,met2
metal2 rmetal2,rm2,rmet2
metal2 via2,m3contact,m3cut,m3c,v2
-metal2 sealvia2,sealv2
metal3 metal3,m3,met3
metal3 rmetal3,rm3,rmet3
metal3 via3,v3
-metal3 sealvia3,sealv3
metal4 metal4,m4,met4
metal4 rmetal4,rm4,rmet4
metal4 via4,v4
-metal4 sealvia4,sealv4
metal5 metal5,m5,met5
metal5 rm5,rmetal5,rmet5
metal5 via5,v5
-metal5 sealvia5,sealv5
mimcap mimcap,mim,capm
mimcap mimcapcontact,mimcapc,mimcc,capmc
metal6 metal6,m6,met6
metal6 rm6,rmetal6,rmet6
metal6 via6,v6
-metal6 sealvia6,sealv6
metal7 metal7,m7,met7
metal7 rm7,rmetal7,rmet7
-metal7 pillar,cu
-metal7 solder,sbump
comment thruvia,tsv
end
cifinput
style riku
templayer large_dnwell DNWELL
and NWELL
shrink 1140
grow 1140
templayer small_dnwell DNWELL
and NWELL
and-not large_dnwell
templayer pnparea DIFF
and PSD
and-not THKOX
and-not SBLK
and-not NSDBLOCK
and-not DIODE
and small_dnwell
grow 1050
and NWELL
layer pnp pnparea
layer nwell NWELL,WELLPIN
and-not pnparea
templayer nwellarea NWELL
and-not pnp
copyup nwelcheck
templayer xnwelcheck nwelcheck
copyup nwelcheck
templayer hvarea THKOX
copyup hvcheck
templayer xhvcheck hvcheck
copyup hvcheck
layer pwell DIFF
and-not PWELLBLK
and-not NWELL,nwelcheck
grow 130
and-not NWELL,nwelcheck
or SUBTXT
grow 420
shrink 420
layer dnwell DNWELL
layer isosub SUBCUT
shrink 400
grow 400
layer tsv THRUVIA
and TSVID
templayer ndiffarea DIFF,DIFFPIN
and-not POLY
and-not NWELL,nwelcheck
and-not PSD
and-not DIODE
and-not THKOX,hvcheck
and-not NSDBLOCK
copyup ndifcheck
layer ndiff ndiffarea
templayer xndifcheck ndifcheck
copyup ndifcheck
templayer hvndiffarea DIFF,DIFFPIN
and-not POLY
and-not NWELL,nwelcheck
and-not PSD
and-not DIODE
and THKOX,hvcheck
and-not NSDBLOCK
copyup hvndifcheck
layer hvndiff hvndiffarea
templayer hvxndifcheck hvndifcheck
copyup hvndifcheck
templayer lvnpnarea EMITTER
and DIFFMASK
grow 890
and DIFFMASK
grow 1600
and DIFFMASK,NSDBLOCK
templayer npnarea EMITTER
and DIFF
and BIPOLARID
grow 705
and DIFF,DIFFMASK
templayer hvnpnarea HVEMITTER
and DIFF
and BIPOLARID
grow 705
and DIFF,DIFFMASK
layer nemitter DIFFMASK
and lvnpnarea
and-not EMITTER
layer nemitter DIFF
and npnarea
and-not EMITTER
layer hvnemitter DIFF
and hvnpnarea
and-not HVEMITTER
layer npn NSDBLOCK
and lvnpnarea
grow 40
layer npn DIFFMASK,DIFF
and npnarea,hvnpnarea
layer gec DIFFMASK
and lvnpnarea
and EMITTER
layer nec DIFF
and npnarea
and EMITTER
layer hvnec DIFF
and hvnpnarea
and HVEMITTER
layer pbc NSDBLOCK
and lvnpnarea
and CONT
layer pbc DIFFMASK
and npnarea,hvnpnarea
and CONT
layer ndiode DIFF
and-not NSDBLOCK
and DIODE
and-not NWELL,nwelcheck
and-not POLY
and-not PSD
and-not THKOX,hvcheck
templayer ndiodearea DIODE
and-not NSDBLOCK
and-not THKOX,hvcheck
and-not NWELL,nwelcheck
copyup DIODE
templayer pdiffarea DIFF,DIFFPIN
and-not POLY
and NWELL,nwelcheck
and-not DIODE
and-not THKOX,hvcheck
and PSD
copyup pdifcheck
layer pdiff pdiffarea
templayer hvpdiffarea DIFF,DIFFPIN
and-not POLY
and NWELL,nwelcheck
and THKOX,hvcheck
and-not DIODE
and PSD
and-not SBLK
copyup hvpdifcheck
layer hvpdiff hvpdiffarea
templayer xpdifcheck pdifcheck
copyup pdifcheck
layer pdiode DIFF
and PSD
and-not POLY
and-not THKOX,hvcheck
and DIODE
templayer pdiodearea DIODE
and PSD
and-not THKOX,hvcheck
copyup DIODE,PSD
templayer isoarea NSDBLOCK
and-not PSD
grow 150
layer hvndiffres DIFF
and SBLK
and THKOX
and-not isoarea
and-not DIODE
and-not BIPOLARID
and-not POLY
layer isodiffres DIFF,DIFFMASK
and isoarea
and SBLK
and-not THKOX
and-not BIPOLARID
and-not POLY
layer hvisodiffres DIFF
and isoarea
and SBLK
and THKOX
and-not BIPOLARID
and-not POLY
layer ndiff DIFFMASK
and isoarea
and SBLK
and-not NWELL
and-not BIPOLARID
grow 150
and DIFFMASK
grow 200
and DIFFMASK
and-not PSD
and-not isoarea
layer psd DIFFMASK
and isoarea
and SBLK
and-not NWELL
and-not BIPOLARID
grow 200
and DIFFMASK
and PSD
and-not SBLK
templayer schottkyarea DIODE
and SBLK
layer schottky schottkyarea
and-not CONT
layer sdic DIODE
and SBLK
and CONT
layer nwell schottkyarea
grow 1000
and DIFF
grow 620
layer pfet DIFF
and POLY
and PSD
and NWELL,nwelcheck
and-not THKOX,hvcheck
templayer hvxpdifcheck hvpdifcheck
copyup hvpdifcheck
templayer hvpfetarea DIFF
and POLY
and PSD
and NWELL,nwelcheck
and THKOX,hvcheck
layer hvpfet hvpfetarea
and-not ESDID
layer hvpfetesd hvpfetarea
and ESDID
layer nfet DIFF
and POLY
and-not PSD
and-not NSDBLOCK
and-not THKOX,hvcheck
templayer nsdarea DIFF
and-not NSDBLOCK
and NWELL,nwelcheck
and-not POLY
and-not PSD
and-not THKOX,hvcheck
copyup nsubcheck
layer nsd nsdarea
templayer xnsubcheck nsubcheck
copyup nsubcheck
templayer psdarea DIFF
and-not DIODE
and PSD
and-not NWELL,nwelcheck
and-not POLY
and-not THKOX,hvcheck
copyup psubcheck
layer psd psdarea
templayer hvnfetarea DIFF
and POLY
and-not PSD
and-not NSDBLOCK
and THKOX,hvcheck
grow 350
layer hvnfetesd DIFF
and POLY
and-not PSD
and-not NSDBLOCK
and THKOX,hvcheck
and ESDID
layer hvnfet DIFF
and POLY
and-not PSD
and-not NSDBLOCK
and THKOX,hvcheck
and-not ESDID
templayer hvnsdarea DIFF
and-not NSDBLOCK
and-not SBLK
and NWELL,nwelcheck
and-not POLY
and-not PSD
and THKOX,hvcheck
copyup hvnsubcheck
layer hvnsd hvnsdarea
templayer hvxnsubcheck hvnsubcheck
copyup hvnsubcheck
templayer hvpsdarea DIFF
and PSD
and-not NWELL,nwelcheck
and-not POLY
and THKOX,hvcheck
copyup hvpsubcheck
layer hvpsd hvpsdarea
templayer xpsubcheck psubcheck
copyup psubcheck
templayer hvxpsubcheck hvpsubcheck
copyup hvpsubcheck
layer poly POLY,POLYPIN
and-not POLYRES
and-not RESDEF
and-not DIFF
layer nres RESDEF
and POLYRES,POLY
and EXTBLOCK
and-not PSD
and-not NSD
layer pres POLYRES,POLY
and EXTBLOCK
and SBLK
and PSD
and-not NSD
layer xres POLYRES
and EXTBLOCK
and SBLK
and PSD
and NSD
templayer xpolycheck polycheck
copyup polycheck
templayer ndcbase CONT
or barecont
and MET1
or barecont
and DIFF
and-not NSDBLOCK
and-not npnarea,hvnpnarea
and-not NWELL,nwelcheck
and-not THKOX,hvcheck
layer ndc ndcbase
grow 85
shrink 85
shrink 85
grow 85
or ndcbase
templayer nscbase CONT
or barecont
and MET1
or barecont
and DIFF
and-not NSDBLOCK
and-not npnarea,hvnpnarea
and NWELL,nwelcheck
and-not THKOX,hvcheck
layer nsc nscbase
grow 85
shrink 85
shrink 85
grow 85
or nscbase
templayer pdcbase CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and NWELL,nwelcheck
and-not THKOX,hvcheck
layer pdc pdcbase
grow 85
shrink 85
shrink 85
grow 85
or pdcbase
templayer pdcnowell CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and-not THKOX,hvcheck
layer pdc pdcnowell
grow 85
shrink 85
shrink 85
grow 85
or pdcnowell
templayer pscbase CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and-not NWELL,nwelcheck
and-not THKOX,hvcheck
layer psc pscbase
and-not EDGESEAL
grow 85
shrink 85
shrink 85
grow 85
or pscbase
layer sealc pscbase
and EDGESEAL
templayer pcbase CONT
or barecont
and MET1
or barecont
and POLY
and-not DIFF
layer pc pcbase
grow 85
shrink 85
shrink 85
grow 85
or pcbase
templayer ndicbase CONT
or barecont
and MET1
or barecont
and DIFF
and-not NSDBLOCK
and DIODE
and-not NWELL,nwelcheck
and-not POLY
and-not PSD
and-not THKOX,hvcheck
layer ndic ndicbase
grow 85
shrink 85
shrink 85
grow 85
or ndicbase
templayer pdicbase CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and DIODE
and-not POLY
and-not THKOX,hvcheck
layer pdic pdicbase
grow 85
shrink 85
shrink 85
grow 85
or pdicbase
templayer hvndcbase CONT
or barecont
and MET1
or barecont
and DIFF
and-not NSDBLOCK
and-not NWELL,nwelcheck
and THKOX,hvcheck
layer hvndc hvndcbase
grow 85
shrink 85
shrink 85
grow 85
or hvndcbase
templayer hvnscbase CONT
or barecont
and MET1
or barecont
and DIFF
and-not NSDBLOCK
and-not SBLK
and NWELL,nwelcheck
and THKOX,hvcheck
layer hvnsc hvnscbase
grow 85
shrink 85
shrink 85
grow 85
or hvnscbase
templayer hvpdcbase CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and NWELL,nwelcheck
and THKOX,hvcheck
layer hvpdc hvpdcbase
grow 85
shrink 85
shrink 85
grow 85
or hvpdcbase
templayer hvpdcnowell CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and THKOX,hvcheck
layer hvpdc hvpdcnowell
grow 85
shrink 85
shrink 85
grow 85
or hvpdcnowell
templayer hvpscbase CONT
or barecont
and MET1
or barecont
and DIFF
and PSD
and-not NWELL,nwelcheck
and THKOX,hvcheck
layer hvpsc hvpscbase
grow 85
shrink 85
shrink 85
grow 85
or hvpscbase
layer	difffill  DIFFFILL
layer	polyfill POLYFILL
layer m1 MET1,MET1TXT,MET1PIN
and-not MET1RES
and-not MET1SLIT
layer iprobe IPROBE
layer diffprobe DPROBE
layer rm1 MET1
and MET1RES
layer m1fill MET1FILL
layer mimcap MET5
and MIM
layer mimcc VIA5,MIMCC
and MIM
grow 60
grow 40
shrink 40
layer m2c VIA1
and-not EDGESEAL
grow 5
grow 105
shrink 105
shrink 95
grow 95
layer sealv1 VIA1
and EDGESEAL
layer m2 MET2,MET2TXT,MET2PIN
and-not MET2RES
and-not MET2SLIT
layer rm2 MET2
and MET2RES
layer m2fill MET2FILL
layer m3c VIA2
and-not EDGESEAL
grow 5
grow 105
shrink 105
shrink 95
grow 95
layer sealv2 VIA2
and EDGESEAL
layer m3 MET3,MET3TXT,MET3PIN
and-not MET3RES
and-not MET3SLIT
layer rm3 MET3
and MET3RES
layer m3fill MET3FILL
layer via3 VIA3
and-not EDGESEAL
grow 5
grow 105
shrink 105
shrink 95
grow 95
layer sealv3 VIA3
and EDGESEAL
layer m4 MET4,MET4TXT,MET4PIN
and-not MET4RES
and-not MET4SLIT
layer rm4 MET4
and MET4RES
layer m4fill MET4FILL
layer via4 VIA4
and-not EDGESEAL
grow 5
grow 105
shrink 105
shrink 95
grow 95
layer sealv4 VIA4
and EDGESEAL
layer m5 MET5,MET5TXT,MET5PIN
and-not MET5RES
and-not MET5SLIT
layer rm5 MET5
and MET5RES
layer m5fill MET5FILL
layer via5 VIA5
and-not EDGESEAL
and-not MIM
grow 100
grow 110
shrink 110
shrink 305
grow 305
layer sealv5 VIA5
and EDGESEAL
layer m6 MET6,MET6TXT,MET6PIN
and-not MET6RES
and-not MET6SLIT
layer rm6 MET6
and MET6RES
layer m6fill MET6FILL
layer via6 VIA6
and-not EDGESEAL
grow 500
grow 25
shrink 25
shrink 945
grow 945
layer sealv6 VIA6
and EDGESEAL
layer m7 MET7,MET7TXT,MET7PIN
and-not MET7RES
and-not MET7SLIT
and-not GLASS
layer rm7 MET7
and MET7RES
layer m7fill MET7FILL
templayer ndiccopy CONT
and LI
and DIODE
and DIFF
and-not NWELL,nwelcheck
and NSD
and-not THKOX,hvcheck
layer ndic ndiccopy
grow 85
shrink 85
shrink 85
grow 85
or ndiccopy
templayer pdiccopy CONT
and LI
and DIODE
and DIFF
and PSD
and-not THKOX,hvcheck
layer pdic pdiccopy
grow 85
shrink 85
shrink 85
grow 85
or pdiccopy
templayer ndccopy CONT
and ndifcheck
layer ndc ndccopy
grow 85
shrink 85
shrink 85
grow 85
or ndccopy
templayer hvndccopy CONT
and hvndifcheck
layer hvndc hvndccopy
grow 85
shrink 85
shrink 85
grow 85
or hvndccopy
templayer pdccopy CONT
and pdifcheck
layer pdc pdccopy
grow 85
shrink 85
shrink 85
grow 85
or pdccopy
templayer hvpdccopy CONT
and hvpdifcheck
layer hvpdc hvpdccopy
grow 85
shrink 85
shrink 85
grow 85
or hvpdccopy
templayer pccopy CONT
and polycheck
layer pc pccopy
grow 85
shrink 85
shrink 85
grow 85
or pccopy
templayer nsccopy CONT
and nsubcheck
layer nsc nsccopy
grow 85
shrink 85
shrink 85
grow 85
or nsccopy
templayer hvnsccopy CONT
and hvnsubcheck
layer hvnsc hvnsccopy
grow 85
shrink 85
shrink 85
grow 85
or hvnsccopy
templayer psccopy CONT
and psubcheck
layer psc psccopy
grow 85
shrink 85
shrink 85
grow 85
or psccopy
templayer hvpsccopy CONT
and hvpsubcheck
layer hvpsc hvpsccopy
grow 85
shrink 85
shrink 85
grow 85
or hvpsccopy
templayer barecont CONT
and MET1
and-not DIFF
and-not POLY
and-not DIODE
and-not nsubcheck
and-not psubcheck
and-not hvnsubcheck
and-not hvpsubcheck
copyup barecont
templayer barecont CONT
and-not MET1
and-not nsubcheck
and-not psubcheck
and-not hvnsubcheck
and-not hvpsubcheck
copyup barecont
layer seal EDGESEAL
grow 7200
and GLASS
layer pillar PILLAR
and MET7
and GLASS
and-not EDGESEAL
layer solder SOLDER
and MET7
and GLASS
and-not EDGESEAL
layer pad PADID
and MET7
and GLASS
and-not EDGESEAL
templayer boundary BOUND
layer comment LVSTEXT
layer fillblock FILLBLOCK
layer obsactive FILLOBSDIFF
and-not DIFF
layer obspoly FILLOBSPOLY
and-not POLY
layer obsm1 FILLOBSM1
and-not MET1
layer obsm2 FILLOBSM2
and-not MET2
layer obsm3 FILLOBSM3
and-not MET3
layer obsm4 FILLOBSM4
and-not MET4
layer obsm5 FILLOBSM5
and-not MET5
layer obsm6 FILLOBSM6
and-not MET6
layer obsm7 FILLOBSM7
and-not MET7
layer hvvar POLY
and DIFF
and-not NSDBLOCK
and-not PSD
and-not CONT
and NWELL,nwelcheck
and THKOX,hvcheck
layer hvvarc CONT
and POLY
and DIFF
and-not NSDBLOCK
and-not PSD
and NWELL,nwelcheck
and THKOX,hvcheck
layer hvpvar POLY
and DIFF
and PSD
and-not NWELL,nwelcheck
and THKOX,hvcheck
calma NWELL 31 0
calma DIFF 1 0
calma DIFFMASK 1 20
calma DNWELL 32 0
calma SUBCUT 40 0
calma NSD 7 0
calma PSD 14 0
calma THKOX 44 0
calma SBLK 28 0
calma RESDEF 24 0
calma POLY 5 0
calma NSDBLOCK 7 21
calma EXTBLOCK 111 0
calma EMITTER 33 0
calma HVEMITTER 156 0
calma DIODE 99 31
calma EMITPOLY 55 0
calma CAPID 99 39
calma ESDID 99 30
calma SRAMID 25 0
calma DIGITALID 16 0
calma HEATRES 52 0
calma HEATTRANS 51 0
calma BIPOLARID 26 0
calma PWELLBLK 46 21
calma DEEPNBLK 32 21
calma EDGESEAL 39 0
calma IPROBE 8 33
calma DPROBE 8 34
calma THRUVIA 152 0
calma TSVID 99 32
calma CONT  6 0
calma MET1  8 0
calma VIA1 19 0
calma MET2 10 0
calma VIA2 29 0
calma MET3 30 0
calma VIA3 49 0
calma MET4 50 0
calma VIA4 66 0
calma MET5 67 0
calma VIA5 125 0
calma MIMCC 129 0
calma MET6 126 0
calma VIA6 133 0
calma MET7 134 0
calma GLASS 9 0
calma PADID 41 0
calma PILLAR 41 35
calma SOLDER 41 36
calma RFMEMS 69 0
calma SUBTXT  40 25
calma MET1TXT 8 25
calma MET2TXT 10 25
calma MET3TXT 30 25
calma MET4TXT 50 25
calma MET5TXT 67 25
calma MET6TXT 126 25
calma MET7TXT 134 25
calma POLYRES 128 0
calma MET1RES 8 29
calma MET2RES 10 29
calma MET3RES 30 29
calma MET4RES 50 29
calma MET5RES 67 29
calma MET6RES 126 29
calma MET7RES 134 29
calma DIFFFILL 1 22
calma POLYFILL 5 22
calma MET1FILL 8 22
calma MET2FILL 10 22
calma MET3FILL 30 22
calma MET4FILL 50 22
calma MET5FILL 67 22
calma MET6FILL 126 22
calma MET7FILL 134 22
calma MET1SLIT 8 24
calma MET2SLIT 10 24
calma MET3SLIT 30 24
calma MET4SLIT 50 24
calma MET5SLIT 67 24
calma MET6SLIT 126 24
calma MET7SLIT 134 24
calma PADPIN 9 2
calma DIFFPIN 1 2
calma POLYPIN 5 2
calma WELLPIN 31 2
calma MET1PIN 8 2
calma MET2PIN 10 2
calma MET3PIN 30 2
calma MET4PIN 50 2
calma MET5PIN 67 2
calma MET6PIN 126 2
calma MET7PIN 134 2
calma BOUND 189 *
calma SEALBOUND 39 4
calma INDBOUND 27 4
calma INDUCTOR 27 0
calma INDPIN 27 2
calma DEVICE 99 0
calma LVSTEXT 63 0
calma MIM 36 0
calma FILLOBSDIFF  1  23
calma FILLOBSPOLY  5  23
calma FILLOBSM1   8  23
calma FILLOBSM2  10 23
calma FILLOBSM3  30 23
calma FILLOBSM4  50 23
calma FILLOBSM5  67 23
calma FILLOBSM6  126 23
calma FILLOBSM7  134 23
calma FILLBLOCK  160 0
calma DIFFFILL	  1  22
calma POLYFILL	  5  22
calma MET1FILL	  8  22
calma MET2FILL	  10  22
calma MET3FILL	  30  22
calma MET4FILL	  50  22
calma MET5FILL	  67  22
calma MET6FILL	  126  22
calma MET7FILL	  134  22
calma NORCX	  148 *
calma DIFFNORCX  1  28
calma POLYNORCX  5  28
calma MET1NORCX  8  28
calma MET2NORCX  10  28
calma MET3NORCX  30  28
calma MET4NORCX  50  28
calma MET5NORCX  67  28
calma MET6NORCX  126  28
calma MET7NORCX  134  28
end
extract
style riku
device msubcircuit sg13_lv_pmos pfet *pdiff *pdiff nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sg13_lv_nmos nfet *ndiff *ndiff pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sg13_hv_pmos hvpfet *hvpdiff *hvpdiff nwell error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sg13_hv_nmos hvnfet *hvndiff *hvndiff pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit sg13_hv_nmos hvnmosesd hvndiffres hvndiffres pwell,space/w error l=l w=w a1=as p1=ps a2=ad p2=pd
device msubcircuit npn13g2 npn gec *ndiff space/w error w1=we l1=le
device msubcircuit npn13g2l npn nec *ndiff space/w error w1=we l1=le
device msubcircuit npn13g2v npn hvnec *ndiff space/w error w1=we l1=le
device msubcircuit pnpMPA pnp *pdiff pwell,space/w w1=we l1=le
device msubcircuit dantenna *ndiode pwell,space/w w=w l=l
device mosfet sg13_lv_pmos pfet *pdiff *pdiff nwell error
device mosfet sg13_lv_nmos nfet *ndiff *ndiff pwell,space/w error
device mosfet sg13_hv_pmos hvpfet *hvpdiff *hvpdiff nwell error
device mosfet sg13_hv_nmos hvnfet *hvndiff *hvndiff pwell,space/w error
end
"#;
