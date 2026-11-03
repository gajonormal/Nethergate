//! Bounded backtracking over declared, exact dependency constraints.
use anyhow::{ensure, Result};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct Requirement {
    pub project: String,
    pub version: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Release {
    pub project: String,
    pub version: String,
    pub required: Vec<Requirement>,
    pub incompatible: Vec<Requirement>,
}
fn matches(r: &Requirement, v: &Release) -> bool {
    r.project == v.project && r.version.as_ref().is_none_or(|id| id == &v.version)
}
pub fn solve(
    roots: &[String],
    mut candidates: impl FnMut(&str) -> Result<Vec<Release>>,
) -> Result<BTreeMap<String, Release>> {
    fn search(
        selected: BTreeMap<String, Release>,
        required: Vec<Requirement>,
        source: &mut impl FnMut(&str) -> Result<Vec<Release>>,
        budget: &mut usize,
    ) -> Result<Option<BTreeMap<String, Release>>> {
        ensure!(
            *budget > 0,
            "A resolução excedeu 512 alternativas; reduz o conjunto e revê as versões"
        );
        *budget -= 1;
        ensure!(
            required.len() <= 4096 && selected.len() <= 64,
            "O conjunto excede o limite de 64 projetos"
        );
        for requirement in &required {
            if let Some(v) = selected.get(&requirement.project) {
                if !matches(requirement, v) {
                    return Ok(None);
                }
            }
        }
        for v in selected.values() {
            for conflict in &v.incompatible {
                if selected
                    .get(&conflict.project)
                    .is_some_and(|other| matches(conflict, other))
                {
                    return Ok(None);
                }
            }
        }
        let next = required.iter().find(|r| !selected.contains_key(&r.project));
        let Some(next) = next else {
            return Ok(Some(selected));
        };
        let options = source(&next.project)?;
        ensure!(options.len() <= 64, "Demasiadas versões candidatas");
        for version in options {
            ensure!(
                version.project == next.project,
                "Projeto inesperado no catálogo"
            );
            if !required
                .iter()
                .filter(|r| r.project == next.project)
                .all(|r| matches(r, &version))
            {
                continue;
            }
            let mut branch = selected.clone();
            let mut needs = required.clone();
            needs.extend(version.required.clone());
            branch.insert(version.project.clone(), version);
            if let Some(result) = search(branch, needs, source, budget)? {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
    ensure!(roots.len() <= 64, "Escolhe até 64 projetos");
    search(BTreeMap::new(),roots.iter().map(|p|Requirement{project:p.clone(),version:None}).collect(),&mut candidates,&mut 512)?
        .ok_or_else(||anyhow::anyhow!("Não existe um conjunto compatível nas versões estáveis consultadas: dependência obrigatória ou conflito declarado. Nenhum ficheiro foi alterado"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(
        project: &str,
        version: &str,
        required: Vec<Requirement>,
        incompatible: Vec<Requirement>,
    ) -> Release {
        Release {
            project: project.into(),
            version: version.into(),
            required,
            incompatible,
        }
    }
    fn q(p: &str, v: Option<&str>) -> Requirement {
        Requirement {
            project: p.into(),
            version: v.map(str::to_string),
        }
    }
    #[test]
    fn resolves_transitive_pins_backtracks_and_accepts_cycles() {
        let catalog = BTreeMap::from([
            (
                "app",
                vec![r("app", "new", vec![q("lib", Some("v1"))], vec![])],
            ),
            (
                "lib",
                vec![
                    r("lib", "v2", vec![], vec![]),
                    r("lib", "v1", vec![q("base", None)], vec![]),
                ],
            ),
            (
                "base",
                vec![
                    r("base", "bad", vec![], vec![q("app", None)]),
                    r("base", "ok", vec![q("app", Some("new"))], vec![]),
                ],
            ),
        ]);
        let result = solve(&["app".into()], |p| Ok(catalog[p].clone())).unwrap();
        assert_eq!(result["lib"].version, "v1");
        assert_eq!(result["base"].version, "ok");
    }
    #[test]
    fn reverse_conflicts_and_inconsistent_pins_block_the_whole_plan() {
        let catalog = BTreeMap::from([
            ("a", vec![r("a", "1", vec![q("lib", Some("1"))], vec![])]),
            ("b", vec![r("b", "1", vec![q("lib", Some("2"))], vec![])]),
            (
                "lib",
                vec![r("lib", "1", vec![], vec![]), r("lib", "2", vec![], vec![])],
            ),
        ]);
        assert!(solve(&["a".into(), "b".into()], |p| Ok(catalog[p].clone())).is_err());
        assert!(solve(&["a".into(), "lib".into()], |p| Ok(vec![if p == "a" {
            r("a", "1", vec![], vec![])
        } else {
            r("lib", "1", vec![], vec![q("a", None)])
        }]))
        .is_err());
    }
}
