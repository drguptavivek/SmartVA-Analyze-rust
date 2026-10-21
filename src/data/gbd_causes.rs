//! Direct Rust translation of `smartva/data/gbd_causes.py`.
#![allow(dead_code)]

pub static GBD_LEVEL1_CAUSE_NAMES: &[(&str, &str)] = &[
    ("A", "Communicable, maternal, neonatal and nutritional diseases"),
    ("B", "Non-communicable diseases"),
    ("C", "Injuries"),
    ("X", "Undetermined")
];

pub static ADULT_GBD_LEVEL1_CAUSES: &[(&str, &str)] = &[
    ("AIDS", "A"),
    ("Bite of Venomous Animal", "C"),
    ("Breast Cancer", "B"),
    ("Cervical Cancer", "B"),
    ("Cirrhosis", "B"),
    ("Colorectal Cancer", "B"),
    ("Diabetes", "B"),
    ("Diarrhea/Dysentery", "A"),
    ("Drowning", "C"),
    ("Epilepsy", "B"),
    ("Esophageal Cancer", "B"),
    ("Falls", "C"),
    ("Fires", "C"),
    ("Homicide", "C"),
    ("Leukemia/Lymphomas", "B"),
    ("Lung Cancer", "B"),
    ("Malaria", "A"),
    ("Maternal", "A"),
    ("Other Cardiovascular Diseases", "B"),
    ("Other Infectious Diseases", "A"),
    ("Other Injuries", "C"),
    ("Other Non-communicable Diseases", "B"),
    ("Pneumonia", "A"),
    ("Poisonings", "C"),
    ("Prostate Cancer", "B"),
    ("Chronic Kidney Disease", "B"),
    ("Road Traffic", "C"),
    ("Stomach Cancer", "B"),
    ("Stroke", "B"),
    ("Suicide", "C"),
    ("TB", "A"),
    ("Chronic Respiratory", "B"),
    ("Ischemic Heart Disease", "B"),
    ("Other Cancers", "B"),
    ("Undetermined", "X")
];

pub static CHILD_GBD_LEVEL1_CAUSES: &[(&str, &str)] = &[
    ("AIDS", "A"),
    ("Bite of Venomous Animal", "C"),
    ("Diarrhea/Dysentery", "A"),
    ("Drowning", "C"),
    ("Encephalitis", "A"),
    ("Falls", "C"),
    ("Fires", "C"),
    ("Hemorrhagic fever", "A"),
    ("Malaria", "A"),
    ("Measles", "A"),
    ("Meningitis", "A"),
    ("Childhood Cancer", "B"),
    ("Childhood Cardiovascular Diseases", "B"),
    ("Other Defined Causes of Child Deaths", "B"),
    ("Digestive Diseases", "B"),
    ("Other Infectious Diseases", "A"),
    ("Pneumonia", "A"),
    ("Poisonings", "C"),
    ("Road Traffic", "C"),
    ("Sepsis", "A"),
    ("Homicide", "C"),
    ("Undetermined", "X")
];

pub static NEONATE_GBD_LEVEL1_CAUSES: &[(&str, &str)] = &[
    ("Birth asphyxia", "A"),
    ("Congenital malformation", "B"),
    ("Neonatal Meningitis/Sepsis", "A"),
    ("Neonatal Pneumonia", "A"),
    ("Preterm Delivery", "A"),
    ("Stillbirth", "A"),
    ("Undetermined", "X")
];


pub fn get_gbd_level1(module: &str, cause34: &str) -> Option<&'static str> {
    let table = match module {
        "adult" => ADULT_GBD_LEVEL1_CAUSES,
        "child" => CHILD_GBD_LEVEL1_CAUSES,
        "neonate" => NEONATE_GBD_LEVEL1_CAUSES,
        _ => return None,
    };
    table.iter().find(|(k, _)| *k == cause34).map(|(_, v)| *v)
}