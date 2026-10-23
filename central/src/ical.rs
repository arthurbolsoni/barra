//! Leitura de agenda no formato iCalendar (RFC 5545), o do endereço secreto do Google Agenda e
//! do calendário publicado do Outlook.com. Expande as recorrências (`RRULE`, `EXDATE`, exceções
//! por `RECURRENCE-ID`) dentro de uma janela de datas e devolve os eventos em hora local.
//!
//! Fuso: hora com `Z` é UTC; hora com `TZID` usa o deslocamento do `VTIMEZONE` correspondente
//! (o do componente STANDARD ou DAYLIGHT em vigor na data); sem `VTIMEZONE`, vale a hora local.

use std::collections::HashMap;

use chrono::{Datelike, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};

pub struct Evento {
    pub uid: String,
    pub titulo: String,
    pub inicio: NaiveDateTime,
    pub fim: NaiveDateTime,
    pub dia_inteiro: bool,
    pub local: String,
    pub descricao: String,
    pub link: String,
    pub reuniao: String,
}

/// Uma linha de propriedade: nome, parâmetros e valor (já sem as quebras de dobra).
struct Prop {
    nome: String,
    params: HashMap<String, String>,
    valor: String,
}

fn linhas(texto: &str) -> Vec<Prop> {
    // desdobra: linha que começa com espaço ou tab continua a anterior
    let mut juntas: Vec<String> = Vec::new();
    for l in texto.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if (l.starts_with(' ') || l.starts_with('\t')) && !juntas.is_empty() {
            juntas.last_mut().unwrap().push_str(&l[1..]);
        } else if !l.is_empty() {
            juntas.push(l.to_string());
        }
    }
    let mut v = Vec::new();
    for l in juntas {
        // o primeiro ':' fora de aspas separa nome/parâmetros do valor
        let mut aspas = false;
        let mut corte = None;
        for (i, c) in l.char_indices() {
            match c {
                '"' => aspas = !aspas,
                ':' if !aspas => {
                    corte = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(i) = corte else { continue };
        let (cab, valor) = (&l[..i], &l[i + 1..]);
        let mut partes = cab.split(';');
        let nome = partes.next().unwrap_or("").to_ascii_uppercase();
        let params = partes
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
            .collect();
        v.push(Prop { nome, params, valor: valor.to_string() });
    }
    v
}

fn texto(v: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in v.chars() {
        if esc {
            out.push(match c {
                'n' | 'N' => '\n',
                outro => outro,
            });
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Deslocamentos de um VTIMEZONE: (início da vigência, deslocamento, regra anual "mês, n-ésimo dia da semana").
struct Fuso {
    trocas: Vec<(NaiveDateTime, i32, Option<(u32, i32, Weekday)>)>,
}

impl Fuso {
    fn deslocamento(&self, dt: NaiveDateTime) -> Option<i32> {
        // candidato de cada componente: a vigência mais recente até `dt` (repetida a cada ano pela regra)
        let mut melhor: Option<(NaiveDateTime, i32)> = None;
        for &(ini, off, regra) in &self.trocas {
            let vig = match regra {
                Some((mes, n, wd)) => {
                    let neste = n_esimo(dt.year(), mes, n, wd).map(|d| d.and_time(ini.time()));
                    match neste {
                        Some(x) if x <= dt => Some(x),
                        _ => n_esimo(dt.year() - 1, mes, n, wd).map(|d| d.and_time(ini.time())),
                    }
                    .filter(|&x| x >= ini)
                }
                None => Some(ini).filter(|&x| x <= dt),
            };
            if let Some(x) = vig {
                if melhor.is_none_or(|(m, _)| x > m) {
                    melhor = Some((x, off));
                }
            }
        }
        melhor.map(|(_, o)| o).or_else(|| self.trocas.first().map(|t| t.1))
    }
}

fn deslocamento(s: &str) -> Option<i32> {
    let (sinal, r) = match s.as_bytes().first()? {
        b'-' => (-1, &s[1..]),
        b'+' => (1, &s[1..]),
        _ => (1, s),
    };
    let h: i32 = r.get(0..2)?.parse().ok()?;
    let m: i32 = r.get(2..4)?.parse().ok()?;
    Some(sinal * (h * 3600 + m * 60))
}

/// n-ésimo dia da semana do mês (n negativo conta do fim).
fn n_esimo(ano: i32, mes: u32, n: i32, wd: Weekday) -> Option<NaiveDate> {
    if n > 0 {
        NaiveDate::from_weekday_of_month_opt(ano, mes, wd, n as u8)
    } else {
        let ultimo = NaiveDate::from_ymd_opt(ano, mes, 1)?.checked_add_months(chrono::Months::new(1))?.pred_opt()?;
        let atras = (7 + ultimo.weekday().num_days_from_monday() as i64 - wd.num_days_from_monday() as i64) % 7;
        let d = ultimo - Duration::days(atras + 7 * (-n as i64 - 1));
        (d.month() == mes).then_some(d)
    }
}

fn dia_semana(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// "2MO", "-1FR", "TU" → (n, dia); n = 0 sem ordinal.
fn byday(s: &str) -> Option<(i32, Weekday)> {
    let s = s.trim();
    let (num, wd) = s.split_at(s.len().checked_sub(2)?);
    let n = if num.is_empty() || num == "+" { 0 } else { num.trim_start_matches('+').parse().ok()? };
    Some((n, dia_semana(wd)?))
}

struct Leitor {
    fusos: HashMap<String, Fuso>,
}

impl Leitor {
    /// Valor de data ou data e hora em hora local; `true` quando é só data (dia inteiro).
    fn data(&self, p: &Prop) -> Option<(NaiveDateTime, bool)> {
        self.data_valor(&p.valor, p.params.get("TZID").map(String::as_str), p.params.get("VALUE").map(String::as_str))
    }

    fn data_valor(&self, v: &str, tzid: Option<&str>, tipo: Option<&str>) -> Option<(NaiveDateTime, bool)> {
        let v = v.trim();
        if tipo == Some("DATE") || v.len() == 8 {
            return Some((NaiveDate::parse_from_str(v, "%Y%m%d").ok()?.and_time(NaiveTime::MIN), true));
        }
        let (corpo, utc) = match v.strip_suffix('Z') {
            Some(c) => (c, true),
            None => (v, false),
        };
        let ingenua = NaiveDateTime::parse_from_str(corpo, "%Y%m%dT%H%M%S").ok()?;
        let off = if utc { Some(0) } else { tzid.and_then(|t| self.fusos.get(t)).and_then(|f| f.deslocamento(ingenua)) };
        let local = match off {
            Some(o) => {
                let fo = FixedOffset::east_opt(o)?;
                fo.from_local_datetime(&ingenua).single()?.with_timezone(&Local).naive_local()
            }
            // TZID sem VTIMEZONE (ou hora "flutuante"): hora local; nomes IANA conhecidos como UTC não entram aqui
            None => match tzid {
                Some(t) if t.eq_ignore_ascii_case("UTC") || t.eq_ignore_ascii_case("Etc/UTC") => Utc.from_utc_datetime(&ingenua).with_timezone(&Local).naive_local(),
                _ => ingenua,
            },
        };
        Some((local, false))
    }
}

struct Bruto {
    props: Vec<Prop>,
}

impl Bruto {
    fn um(&self, nome: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.nome == nome)
    }
    fn texto(&self, nome: &str) -> String {
        self.um(nome).map(|p| texto(&p.valor)).unwrap_or_default()
    }
}

/// Endereço de reunião (Meet, Teams, Zoom) no campo próprio ou no meio da descrição.
fn reuniao(b: &Bruto, descricao: &str) -> String {
    for campo in ["X-GOOGLE-CONFERENCE", "X-MICROSOFT-SKYPETEAMSMEETINGURL", "X-MICROSOFT-ONLINEMEETINGEXTERNALLINK"] {
        let v = b.texto(campo);
        if !v.is_empty() {
            return v;
        }
    }
    for padrao in ["https://meet.google.com/", "https://teams.microsoft.com/", "https://teams.live.com/", "zoom.us/j/"] {
        if let Some(i) = descricao.find(padrao) {
            // o endereço começa no "http" mais próximo antes do padrão (o do Zoom vem com subdomínio)
            let Some(ini) = descricao[..i + padrao.len()].rfind("http") else { continue };
            let resto = &descricao[ini..];
            let fim = resto.find(|c: char| c.is_whitespace() || matches!(c, '>' | '<' | ')' | '"' | '\'')).unwrap_or(resto.len());
            return resto[..fim].to_string();
        }
    }
    String::new()
}

/// Lê o calendário e devolve os eventos que tocam o intervalo [de, ate).
pub fn eventos(conteudo: &str, de: NaiveDate, ate: NaiveDate) -> Vec<Evento> {
    let props = linhas(conteudo);
    // separa VTIMEZONE e VEVENT
    let mut fusos: HashMap<String, Fuso> = HashMap::new();
    let mut brutos: Vec<Bruto> = Vec::new();
    let mut pilha: Vec<String> = Vec::new();
    let mut atual: Vec<Prop> = Vec::new();
    let mut tz_id = String::new();
    let mut tz_comp: Vec<Prop> = Vec::new();
    for p in props {
        match p.nome.as_str() {
            "BEGIN" => {
                pilha.push(p.valor.to_ascii_uppercase());
                if pilha.last().is_some_and(|c| c == "VEVENT") {
                    atual.clear();
                }
                if pilha.last().is_some_and(|c| c == "STANDARD" || c == "DAYLIGHT") {
                    tz_comp.clear();
                }
            }
            "END" => {
                let c = pilha.pop().unwrap_or_default();
                match c.as_str() {
                    "VEVENT" => brutos.push(Bruto { props: std::mem::take(&mut atual) }),
                    "STANDARD" | "DAYLIGHT" => {
                        let achar = |n: &str| tz_comp.iter().find(|x| x.nome == n).map(|x| x.valor.clone());
                        let ini = achar("DTSTART").and_then(|v| NaiveDateTime::parse_from_str(v.trim(), "%Y%m%dT%H%M%S").ok()).unwrap_or(NaiveDateTime::MIN);
                        let off = achar("TZOFFSETTO").and_then(|v| deslocamento(v.trim()));
                        // regra anual (FREQ=YEARLY;BYMONTH=..;BYDAY=..)
                        let regra = achar("RRULE").and_then(|r| {
                            let kv: HashMap<&str, &str> = r.split(';').filter_map(|x| x.split_once('=')).collect();
                            let mes: u32 = kv.get("BYMONTH")?.parse().ok()?;
                            let (n, wd) = byday(kv.get("BYDAY")?)?;
                            Some((mes, if n == 0 { 1 } else { n }, wd))
                        });
                        if let Some(o) = off {
                            fusos.entry(tz_id.clone()).or_insert(Fuso { trocas: Vec::new() }).trocas.push((ini, o, regra));
                        }
                    }
                    _ => {}
                }
            }
            "TZID" if pilha.last().is_some_and(|c| c == "VTIMEZONE") => tz_id = p.valor.clone(),
            _ => match pilha.last().map(String::as_str) {
                Some("VEVENT") => atual.push(p),
                Some("STANDARD") | Some("DAYLIGHT") => tz_comp.push(p),
                _ => {}
            },
        }
    }
    let leitor = Leitor { fusos };

    // exceções de série: (uid, início original) → evento que substitui a ocorrência
    let mut excecoes: HashMap<(String, NaiveDateTime), usize> = HashMap::new();
    for (i, b) in brutos.iter().enumerate() {
        if let Some(r) = b.um("RECURRENCE-ID") {
            if let Some((dt, _)) = leitor.data(r) {
                excecoes.insert((b.texto("UID"), dt), i);
            }
        }
    }

    let janela_ini = de.and_time(NaiveTime::MIN);
    let janela_fim = ate.and_time(NaiveTime::MIN);
    let mut saida = Vec::new();
    for b in &brutos {
        if b.texto("STATUS").eq_ignore_ascii_case("CANCELLED") {
            continue;
        }
        let Some((ini, dia_inteiro)) = b.um("DTSTART").and_then(|p| leitor.data(p)) else { continue };
        let dur = match b.um("DTEND").and_then(|p| leitor.data(p)) {
            Some((f, _)) => f - ini,
            None => b.um("DURATION").and_then(|p| duracao(&p.valor)).unwrap_or(if dia_inteiro { Duration::days(1) } else { Duration::zero() }),
        };
        let uid = b.texto("UID");
        let descricao = b.texto("DESCRIPTION");
        let base = Evento {
            uid: uid.clone(),
            titulo: Some(b.texto("SUMMARY")).filter(|s| !s.is_empty()).unwrap_or_else(|| "(sem título)".into()),
            inicio: ini,
            fim: ini + dur,
            dia_inteiro,
            local: b.texto("LOCATION"),
            reuniao: reuniao(b, &descricao),
            descricao,
            link: b.texto("URL"),
        };
        let inicios: Vec<NaiveDateTime> = match b.um("RRULE").filter(|_| b.um("RECURRENCE-ID").is_none()) {
            None => vec![ini],
            Some(r) => {
                let mut fora: Vec<NaiveDateTime> = Vec::new();
                for p in b.props.iter().filter(|p| p.nome == "EXDATE") {
                    let tz = p.params.get("TZID").map(String::as_str);
                    let tipo = p.params.get("VALUE").map(String::as_str);
                    fora.extend(p.valor.split(',').filter_map(|v| leitor.data_valor(v, tz, tipo)).map(|(d, _)| d));
                }
                let mut v = expandir(&r.valor, ini, janela_fim, &leitor);
                v.retain(|d| !fora.contains(d) && !(dia_inteiro && fora.iter().any(|f| f.date() == d.date())));
                // ocorrências substituídas por exceção saem daqui (a exceção entra como evento próprio)
                v.retain(|d| !excecoes.contains_key(&(uid.clone(), *d)));
                v
            }
        };
        for i0 in inicios {
            let fim = i0 + dur;
            let toca = if dia_inteiro { i0 < janela_fim && fim > janela_ini } else { i0 < janela_fim && fim.max(i0 + Duration::minutes(1)) > janela_ini };
            if toca {
                saida.push(Evento { inicio: i0, fim, uid: base.uid.clone(), titulo: base.titulo.clone(), local: base.local.clone(), descricao: base.descricao.clone(), link: base.link.clone(), reuniao: base.reuniao.clone(), dia_inteiro });
            }
        }
    }
    saida.sort_by_key(|e| e.inicio);
    saida
}

/// "PT1H30M", "P1D", "-PT15M".
fn duracao(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (sinal, v) = match v.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, v.trim_start_matches('+')),
    };
    let v = v.strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    for c in v.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => {}
            u => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match u {
                    'W' => 7 * 86400,
                    'D' => 86400,
                    'H' => 3600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(Duration::seconds(sinal * total))
}

/// Inícios das ocorrências de uma regra, de `ini` até `limite`.
fn expandir(regra: &str, ini: NaiveDateTime, limite: NaiveDateTime, leitor: &Leitor) -> Vec<NaiveDateTime> {
    let kv: HashMap<String, String> = regra.split(';').filter_map(|x| x.split_once('=')).map(|(k, v)| (k.to_ascii_uppercase(), v.to_string())).collect();
    let freq = kv.get("FREQ").map(String::as_str).unwrap_or("");
    let intervalo: i64 = kv.get("INTERVAL").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let contagem: Option<usize> = kv.get("COUNT").and_then(|v| v.parse().ok());
    let ate = kv.get("UNTIL").and_then(|u| leitor.data_valor(u, None, None)).map(|(d, so_data)| if so_data { d + Duration::days(1) - Duration::seconds(1) } else { d });
    let dias: Vec<(i32, Weekday)> = kv.get("BYDAY").map(|v| v.split(',').filter_map(byday).collect()).unwrap_or_default();
    let dias_mes: Vec<i32> = kv.get("BYMONTHDAY").map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let meses: Vec<u32> = kv.get("BYMONTH").map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let hora = ini.time();
    let d0 = ini.date();

    let mut v: Vec<NaiveDateTime> = Vec::new();
    let mut emitidas = 0usize;
    // devolve false quando acabou (contagem, UNTIL ou limite da janela)
    let mut emitir = |d: NaiveDate, v: &mut Vec<NaiveDateTime>| -> bool {
        let dt = d.and_time(hora);
        if dt < ini {
            return true;
        }
        if ate.is_some_and(|a| dt > a) || dt >= limite || contagem.is_some_and(|c| emitidas >= c) {
            return false;
        }
        emitidas += 1;
        v.push(dt);
        true
    };
    // dias de um mês segundo BYMONTHDAY/BYDAY (ou o dia do início)
    let dias_do_mes = |ano: i32, mes: u32| -> Vec<NaiveDate> {
        let mut r: Vec<NaiveDate> = Vec::new();
        let ultimo = NaiveDate::from_ymd_opt(ano, mes, 1).and_then(|p| p.checked_add_months(chrono::Months::new(1))).and_then(|p| p.pred_opt()).map(|d| d.day() as i32).unwrap_or(28);
        for &md in &dias_mes {
            let dia = if md < 0 { ultimo + 1 + md } else { md };
            if let Some(d) = NaiveDate::from_ymd_opt(ano, mes, dia.max(1) as u32).filter(|_| dia >= 1 && dia <= ultimo) {
                r.push(d);
            }
        }
        for &(n, wd) in &dias {
            if n != 0 {
                r.extend(n_esimo(ano, mes, n, wd));
            } else {
                let mut d = NaiveDate::from_ymd_opt(ano, mes, 1);
                while let Some(x) = d.filter(|x| x.month() == mes) {
                    if x.weekday() == wd {
                        r.push(x);
                    }
                    d = x.succ_opt();
                }
            }
        }
        if dias_mes.is_empty() && dias.is_empty() {
            r.extend(NaiveDate::from_ymd_opt(ano, mes, d0.day()));
        }
        r.sort();
        r.dedup();
        r
    };

    const MAX: usize = 5000;
    match freq {
        "DAILY" => {
            let mut d = d0;
            for _ in 0..MAX * 10 {
                let ok_dia = dias.is_empty() || dias.iter().any(|&(_, wd)| wd == d.weekday());
                let ok_mes = meses.is_empty() || meses.contains(&d.month());
                if ok_dia && ok_mes && !emitir(d, &mut v) {
                    break;
                }
                d += Duration::days(intervalo);
                if d.and_time(hora) >= limite {
                    break;
                }
            }
        }
        "WEEKLY" => {
            let semana0 = d0 - Duration::days(d0.weekday().num_days_from_monday() as i64);
            let ds: Vec<Weekday> = if dias.is_empty() { vec![d0.weekday()] } else { dias.iter().map(|&(_, w)| w).collect() };
            let mut semana = semana0;
            'fora: for _ in 0..MAX {
                let mut daquela: Vec<NaiveDate> = ds.iter().map(|w| semana + Duration::days(w.num_days_from_monday() as i64)).collect();
                daquela.sort();
                for d in daquela {
                    if !emitir(d, &mut v) {
                        break 'fora;
                    }
                }
                semana += Duration::days(7 * intervalo);
                if semana.and_time(hora) >= limite {
                    break;
                }
            }
        }
        "MONTHLY" => {
            let mut p = NaiveDate::from_ymd_opt(d0.year(), d0.month(), 1).unwrap_or(d0);
            'fora: for _ in 0..MAX {
                if meses.is_empty() || meses.contains(&p.month()) {
                    for d in dias_do_mes(p.year(), p.month()) {
                        if !emitir(d, &mut v) {
                            break 'fora;
                        }
                    }
                }
                let Some(n) = p.checked_add_months(chrono::Months::new(intervalo as u32)) else { break };
                p = n;
                if p.and_time(hora) >= limite {
                    break;
                }
            }
        }
        "YEARLY" => {
            let ms: Vec<u32> = if meses.is_empty() { vec![d0.month()] } else { meses.clone() };
            let mut ano = d0.year();
            'fora: for _ in 0..MAX {
                for &m in &ms {
                    for d in dias_do_mes(ano, m) {
                        if !emitir(d, &mut v) {
                            break 'fora;
                        }
                    }
                }
                ano += intervalo as i32;
                if NaiveDate::from_ymd_opt(ano, 1, 1).is_none_or(|d| d.and_time(hora) >= limite) {
                    break;
                }
            }
        }
        _ => v.push(ini),
    }
    v
}

#[cfg(test)]
mod testes {
    use super::*;

    const CAL: &str = "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:E. South America Standard Time\r\nBEGIN:STANDARD\r\nDTSTART:16010101T000000\r\nTZOFFSETFROM:-0300\r\nTZOFFSETTO:-0300\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Daily\r\nDTSTART;TZID=E. South America Standard Time:20261019T090000\r\nDTEND;TZID=E. South America Standard Time:20261019T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\nEXDATE;TZID=E. South America Standard Time:20261021T090000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:a\r\nRECURRENCE-ID;TZID=E. South America Standard Time:20261023T090000\r\nSUMMARY:Daily (movida)\r\nDTSTART;TZID=E. South America Standard Time:20261023T100000\r\nDTEND;TZID=E. South America Standard Time:20261023T101500\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Feriado\r\nDTSTART;VALUE=DATE:20261102\r\nDTEND;VALUE=DATE:20261103\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Reunião\r\nDESCRIPTION:Entrar: https://meet.google.com/abc-defg-hij\\nPauta\r\nDTSTART:20261026T130000Z\r\nDTEND:20261026T140000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:Mensal\r\nDTSTART:20260105T120000Z\r\nDURATION:PT30M\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=12\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn expande() {
        let de = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let ate = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let v = eventos(CAL, de, ate);
        let lista: Vec<String> = v.iter().map(|e| format!("{} {} {}", e.inicio.format("%d/%m %H:%M"), e.titulo, e.reuniao)).collect();
        for l in &lista {
            println!("{l}");
        }
        assert!(lista.contains(&"19/10 09:00 Daily ".to_string()));
        assert!(!lista.iter().any(|l| l.starts_with("21/10 09:00")), "EXDATE");
        assert!(lista.contains(&"23/10 10:00 Daily (movida) ".to_string()));
        assert!(!lista.contains(&"23/10 09:00 Daily ".to_string()), "exceção substitui");
        assert!(lista.contains(&"30/10 09:00 Daily ".to_string()));
        assert!(lista.iter().any(|l| l.starts_with("02/11 00:00 Feriado")));
        assert!(lista.iter().any(|l| l.contains("https://meet.google.com/abc-defg-hij")));
        assert!(lista.iter().any(|l| l.starts_with("30/10") && l.contains("Mensal")), "último sexta do mês");
    }
}
