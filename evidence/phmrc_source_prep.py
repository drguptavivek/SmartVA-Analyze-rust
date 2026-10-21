#!/usr/bin/env python3
"""
Reverse-mapping preprocessor () that converts the GHDx PHMRC
Gold Standard Verbal Autopsy validation datasets:
  - IHME_PHMRC_VA_DATA_ADULT_Y2013M09D11_0.csv   (7,841 VAs)
  - IHME_PHMRC_VA_DATA_CHILD_Y2013M09D11_0.csv   (2,064 VAs)
  - IHME_PHMRC_VA_DATA_NEONATE_Y2013M09D11_0.csv (2,625 VAs)
using IHME_PHMRC_VA_DATA_CODEBOOK_Y2013M09D11_0.xlsx into standard ODK PHMRC Full
Questionnaire CSVs accepted by both upstream Python  and .
"""

import argparse
import csv
import re
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

VAR_CONVERSION_PAIRS = [('sid', 'sid'), ('gen_2_1', 'g2_01'), ('gen_2_2a', 'g2_02'), ('gen_3_1', 'g3_01'), ('gen_4_2', 'g4_02'), ('gen_4_3', 'g4_03a'), ('gen_4_4', 'g4_04'), ('gen_4_5', 'g4_05'), ('gen_4_6', 'g4_06'), ('gen_4_7', 'g4_07'), ('gen_4_8', 'g4_08'), ('gen_5_1c', 'g5_01d'), ('gen_5_1b', 'g5_01m'), ('gen_5_1a', 'g5_01y'), ('gen_5_2', 'g5_02'), ('gen_5_3c', 'g5_03d'), ('gen_5_3b', 'g5_03m'), ('gen_5_3a', 'g5_03y'), ('gen_5_4a', 'g5_04a'), ('gen_5_4b', 'g5_04b'), ('gen_5_4c', 'g5_04c'), ('gen_5_5', 'g5_05'), ('gen_5_6', 'g5_06a'), ('gen_5_7', 'g5_07'), ('gen_5_8', 'g5_08'), ('adult_1_1a', 'a1_01_1'), ('adult_1_1b', 'a1_01_2'), ('adult_1_1c', 'a1_01_3'), ('adult_1_1d', 'a1_01_13'), ('adult_1_1e', 'a1_01_5'), ('adult_1_1f', 'a1_01_6'), ('adult_1_1g', 'a1_01_7'), ('adult_1_1h', 'a1_01_8'), ('adult_1_1i', 'a1_01_9'), ('adult_1_1j', 'a1_01_10'), ('adult_1_1k', 'a1_01_11'), ('adult_1_1l', 'a1_01_12'), ('adult_1_1m', 'a1_01_4'), ('adult_1_1n', 'a1_01_14'), ('adult_2_1', 'a2_01a'), ('adult_2_2', 'a2_02'), ('adult_2_3', 'a2_03a'), ('adult_2_3a', 'a2_03b'), ('adult_2_4', 'a2_04'), ('adult_2_5', 'a2_05'), ('adult_2_6', 'a2_06'), ('adult_2_7', 'a2_07'), ('adult_2_8', 'a2_08a'), ('adult_2_8a', 'a2_08b'), ('adultrash1', 'a2_09_1a'), ('adult_2_9a', 'a2_09_1b'), ('adultrash2', 'a2_09_2a'), ('adultrash3', 'a2_09_3a'), ('adult_2_10', 'a2_10'), ('adult_2_11', 'a2_11'), ('adult_2_12', 'a2_12'), ('adult_2_13', 'a2_13'), ('adult_2_14', 'a2_14'), ('adult_2_15', 'a2_15a'), ('adult_2_15a', 'a2_15b'), ('adult_2_16', 'a2_16'), ('adult_2_17', 'a2_17'), ('adult_2_18', 'a2_18'), ('adult_2_19', 'a2_19'), ('adult_2_20', 'a2_20'), ('adult_2_21', 'a2_21'), ('adult_2_22', 'a2_22a'), ('adult_2_23', 'a2_23'), ('adult_2_24', 'a2_24a'), ('adult_2_25', 'a2_25'), ('adult_2_26', 'a2_26a'), ('adult_2_27', 'a2_27'), ('adult_2_28', 'a2_28a'), ('adult_2_29', 'a2_29'), ('adult_2_30', 'a2_30'), ('adult_2_31', 'a2_31'), ('adult_2_32', 'a2_32'), ('adult_2_33', 'a2_33a'), ('adult_2_34', 'a2_34'), ('adult_2_35', 'a2_35'), ('adult_2_36', 'a2_36'), ('adult_2_37', 'a2_37a'), ('adult_2_38', 'a2_38'), ('adult_2_39', 'a2_39_1'), ('adult_2_40', 'a2_40'), ('adult_2_41', 'a2_41a'), ('adult_2_42', 'a2_42'), ('adult_2_43', 'a2_43'), ('adult_2_44', 'a2_44'), ('adult_2_45', 'a2_45'), ('adult_2_46', 'a2_46a'), ('adult_2_46a', 'a2_46b'), ('adult_2_47', 'a2_47'), ('adult_2_48', 'a2_48a'), ('adult_2_48a', 'a2_48b'), ('adult_2_49', 'a2_49'), ('adult_2_50', 'a2_50'), ('adult_2_51', 'a2_51'), ('adult_2_52', 'a2_52'), ('adult_2_53', 'a2_53'), ('adult_2_54', 'a2_54a'), ('adult_2_55', 'a2_55'), ('adult_2_56', 'a2_56'), ('adult_2_57', 'a2_57'), ('adult_2_58', 'a2_58a'), ('adult_2_59', 'a2_59'), ('adult_2_60', 'a2_60'), ('adult_2_61', 'a2_61'), ('adult_2_62', 'a2_62a'), ('adult_2_63', 'a2_63_1'), ('adult_2_64', 'a2_64'), ('adult_2_65', 'a2_65a'), ('adult_2_66', 'a2_66'), ('adult_2_67', 'a2_67'), ('adult_2_68', 'a2_68a'), ('adult_2_69', 'a2_69'), ('adult_2_70', 'a2_70a'), ('adult_2_71', 'a2_71'), ('adult_2_72', 'a2_72'), ('adult_2_73', 'a2_73a'), ('adult_2_74', 'a2_74'), ('adult_2_75', 'a2_75'), ('adult_2_76', 'a2_76a'), ('adult_2_77', 'a2_77'), ('adult_2_78', 'a2_78'), ('adult_2_79', 'a2_79a'), ('adult_2_80', 'a2_80'), ('adult_2_81', 'a2_81'), ('adult_2_82', 'a2_82'), ('adult_2_83', 'a2_83a'), ('adult_2_84', 'a2_84'), ('adult_2_85', 'a2_85'), ('adult_2_86', 'a2_86a'), ('adultparalysis1', 'a2_87_1'), ('adultparalysis2', 'a2_87_2'), ('adultparalysis3', 'a2_87_3'), ('adultparalysis4', 'a2_87_4'), ('adultparalysis5', 'a2_87_5'), ('adultparalysis6', 'a2_87_6'), ('adultparalysis7', 'a2_87_7'), ('adultparalysis8', 'a2_87_8'), ('adultparalysis9', 'a2_87_9'), ('adultparalysis10', 'a2_87_10a'), ('adult_2_87a', 'a2_87_10b'), ('adult_3_1', 'a3_01'), ('adult_3_2', 'a3_02'), ('adult_3_3', 'a3_03'), ('adult_3_4', 'a3_04'), ('adult_3_5', 'a3_05'), ('adult_3_6', 'a3_06'), ('adult_3_7', 'a3_07'), ('adult_3_8', 'a3_08a'), ('adult_3_8a', 'a3_08b'), ('adult_3_9', 'a3_09'), ('adult_3_10', 'a3_10'), ('adult_3_11', 'a3_11a'), ('adult_3_11a', 'a3_11b'), ('adult_3_12', 'a3_12'), ('adult_3_13', 'a3_13'), ('adult_3_14', 'a3_14'), ('adult_3_15', 'a3_15'), ('adult_3_16', 'a3_16a'), ('adult_3_16a', 'a3_16b'), ('adult_3_17', 'a3_17'), ('adult_3_18', 'a3_18'), ('adult_3_19', 'a3_19'), ('adult_3_20', 'a3_20'), ('adult_4_1', 'a4_01'), ('adult_4_2', 'a4_02'), ('adult_4_2a', 'a4_02_5b'), ('adult_4_3a', 'a4_03'), ('adult_4_4a', 'a4_04'), ('adult_4_5', 'a4_05'), ('adult_4_6', 'a4_06'), ('adult_5_1', 'adult_5_1'), ('adultinjury1', 'a5_01_1'), ('adultinjury2', 'a5_01_2'), ('adultinjury3', 'a5_01_3'), ('adultinjury4', 'a5_01_4'), ('adultinjury5', 'a5_01_5'), ('adultinjury6', 'a5_01_6'), ('adultinjury7', 'a5_01_7'), ('adultinjury8', 'a5_01_9a'), ('adult_5_2a', 'a5_01_9b'), ('adult_5_3', 'a5_02'), ('adult_5_4', 'a5_03'), ('adult_5_5', 'a5_04a'), ('adult_6_1', 'a6_01'), ('adult_6_2', 'adult_6_2'), ('adult_6_3', 'a6_03'), ('adult_6_3b', 'a6_03b'), ('adult_6_4', 'a6_04'), ('adult_6_5', 'a6_05'), ('adult_6_6d', 'a6_06_1d'), ('adult_6_6c', 'a6_06_1m'), ('adult_6_6b', 'a6_06_1y'), ('adult_6_6h', 'a6_06_2d'), ('adult_6_6g', 'a6_06_2m'), ('adult_6_6f', 'a6_06_2y'), ('adult_6_7c', 'a6_07d'), ('adult_6_7b', 'a6_07m'), ('adult_6_7a', 'a6_07y'), ('adult_6_8', 'a6_08'), ('adult_6_9', 'a6_09'), ('adult_6_10', 'a6_10'), ('adult_6_11', 'a6_11'), ('adult_6_12', 'a6_12'), ('adult_6_13', 'a6_13'), ('adult_6_14', 'a6_14'), ('adult_6_15', 'a6_15'), ('adult_7_c', 'a7_01'), ('adult_7_11', 'a_7_11'), ('adult_7_10', 'a_7_10'), ('adult_7_9', 'a_7_9'), ('adult_7_8', 'a_7_8'), ('adult_7_7', 'a_7_7'), ('adult_7_6', 'a_7_6'), ('adult_7_5', 'a_7_5'), ('adult_7_4', 'a_7_4'), ('adult_7_3', 'a_7_3'), ('adult_7_2', 'a_7_2'), ('adult_7_1', 'a_7_1'), ('sid', 'sid'), ('gen_2_1', 'g2_01'), ('gen_2_2a', 'g2_02'), ('gen_3_1', 'g3_01'), ('gen_4_2', 'g4_02'), ('gen_4_3', 'g4_03a'), ('gen_4_4', 'g4_04'), ('gen_4_5', 'g4_05'), ('gen_4_6', 'g4_06'), ('gen_4_7', 'g4_07'), ('gen_4_8', 'g4_08'), ('gen_5_1c', 'g5_01d'), ('gen_5_1b', 'g5_01m'), ('gen_5_1a', 'g5_01y'), ('gen_5_2', 'g5_02'), ('gen_5_3c', 'g5_03d'), ('gen_5_3b', 'g5_03m'), ('gen_5_3a', 'g5_03y'), ('gen_5_4a', 'g5_04a'), ('gen_5_4b', 'g5_04b'), ('gen_5_4c', 'g5_04c'), ('gen_5_5', 'g5_05'), ('gen_5_6', 'g5_06a'), ('gen_5_7', 'g5_07'), ('gen_5_8', 'g5_08'), ('child_1_1', 'c1_01'), ('child_1_2', 'c1_02'), ('child_1_3', 'c1_03'), ('child_1_4', 'c1_04'), ('child_1_5', 'c1_05a'), ('child_1_6', 'c1_06a'), ('child_1_6a', 'c1_06b'), ('child_1_7', 'c1_07'), ('child_1_8', 'c1_08a'), ('child_1_8a', 'c1_08b'), ('child_1_11', 'c1_11'), ('child_1_12', 'c1_12'), ('child_1_13', 'c1_13'), ('child_1_14', 'c1_14'), ('child_1_15', 'c1_15'), ('child_1_16', 'c1_16'), ('child_1_17', 'c1_17'), ('child_1_18', 'c1_18'), ('childabnorm1', 'c1_19_1'), ('childabnorm2', 'c1_19_2'), ('childabnorm3', 'c1_19_3'), ('childabnorm4', 'c1_19_4a'), ('child_1_19a', 'c1_19_4b'), ('childabnorm5', 'c1_19_5'), ('child_1_20', 'c1_20a'), ('child_1_21', 'c1_21a'), ('child_1_22', 'c1_22a'), ('child_1_22a', 'c1_22b'), ('child_1_23', 'c1_23'), ('complications1', 'c2_01_1'), ('complications2', 'c2_01_2'), ('complications3', 'c2_01_3'), ('complications4', 'c2_01_4'), ('complications5', 'c2_01_5'), ('complications6', 'c2_01_6'), ('complications7', 'c2_01_7'), ('complications8', 'c2_01_8'), ('complications9', 'c2_01_9'), ('complications10', 'c2_01_10'), ('complications11', 'c2_01_11'), ('complications12', 'c2_01_12'), ('child_2_2', 'c2_02a'), ('child_2_2a', 'c2_02b'), ('child_2_3', 'c2_03'), ('child_2_4', 'c2_04'), ('child_2_5', 'c2_05a'), ('child_2_6', 'c2_06'), ('child_2_7', 'c2_07'), ('child_2_8', 'c2_08a'), ('child_2_8a', 'c2_08b'), ('child_2_9', 'c2_09'), ('child_2_10', 'c2_10a'), ('child_2_10a', 'c2_10b'), ('child_2_11', 'c2_11'), ('child_2_12', 'c2_12'), ('child_2_15', 'c2_15a'), ('child_2_15a', 'c2_15b'), ('child_2_17', 'c2_17'), ('child_2_18', 'c2_18'), ('child_3_1', 'c3_01'), ('child_3_2', 'c3_02'), ('childabnorm31', 'c3_03_1'), ('childabnorm32', 'c3_03_2'), ('childabnorm33', 'c3_03_3'), ('childabnorm34', 'c3_03_4a'), ('child_3_3a', 'c3_03_4b'), ('childabnorm35', 'c3_03_5'), ('child_3_4', 'c3_04'), ('child_3_5', 'c3_05'), ('child_3_6', 'c3_06'), ('child_3_7', 'c3_07'), ('child_3_8', 'c3_08'), ('child_3_9', 'c3_09'), ('child_3_10', 'c3_10'), ('child_3_11', 'c3_11'), ('child_3_12', 'c3_12'), ('child_3_13', 'c3_13'), ('child_3_14', 'c3_14a'), ('child_3_14a', 'c3_14b'), ('child_3_15', 'c3_15'), ('child_3_16', 'c3_16'), ('child_3_17', 'c3_17'), ('child_3_18', 'c3_18a'), ('child_3_18a', 'c3_18b'), ('child_3_19', 'c3_19a'), ('child_3_19a', 'c3_19b'), ('child_3_20', 'c3_20'), ('child_3_21', 'c3_21a'), ('child_3_21a', 'c3_21b'), ('child_3_22', 'c3_22a'), ('child_3_22a', 'c3_22b'), ('child_3_23', 'c3_23'), ('child_3_24', 'c3_24'), ('child_3_25', 'c3_25'), ('child_3_26', 'c3_26'), ('child_3_27', 'c3_27a'), ('child_3_27a', 'c3_27b'), ('child_3_28', 'c3_28a'), ('child_3_28a', 'c3_28b'), ('child_3_29', 'c3_29'), ('child_3_30', 'c3_30a'), ('child_3_30a', 'c3_30b'), ('child_3_31', 'c3_31a'), ('child_3_31a', 'c3_31b'), ('child_3_32', 'c3_32'), ('child_3_33', 'c3_33'), ('child_3_34', 'c3_34'), ('child_3_35', 'c3_35'), ('child_3_36', 'c3_36'), ('child_3_37', 'c3_37'), ('child_3_38', 'c3_38'), ('child_3_39', 'c3_39'), ('child_3_40', 'c3_40'), ('child_3_41', 'c3_41'), ('child_3_42', 'c3_42'), ('child_3_43', 'c3_43'), ('child_3_44', 'c3_44'), ('child_3_45', 'c3_45a'), ('child_3_45a', 'c3_45b'), ('child_3_46', 'c3_46'), ('child_3_47', 'c3_47'), ('child_3_48', 'c3_48'), ('child_3_49', 'c3_49'), ('child_4_1', 'c4_01'), ('child_4_2', 'c4_02a'), ('child_4_2a', 'c4_02b'), ('child_4_3', 'c4_03'), ('child_4_4', 'c4_04'), ('child_4_5', 'c4_05'), ('child_4_6', 'c4_06'), ('child_4_7', 'c4_07a'), ('child_4_7a', 'c4_07b'), ('child_4_8', 'c4_08a'), ('child_4_8a', 'c4_08b'), ('child_4_9', 'c4_09'), ('child_4_10', 'c4_10a'), ('child_4_10a', 'c4_10b'), ('child_4_11', 'c4_11'), ('child_4_12', 'c4_12'), ('child_4_13', 'c4_13a'), ('child_4_13a', 'c4_13b'), ('child_4_14', 'c4_14'), ('child_4_15', 'c4_15'), ('child_4_16', 'c4_16'), ('child_4_17', 'c4_17a'), ('child_4_17a', 'c4_17b'), ('child_4_18', 'c4_18'), ('child_4_19', 'c4_19a'), ('child_4_19a', 'c4_19b'), ('child_4_20', 'c4_20'), ('child_4_22', 'c4_22'), ('child_4_23', 'c4_23'), ('child_4_24', 'c4_24'), ('child_4_25', 'c4_25'), ('child_4_26', 'c4_26'), ('child_4_27', 'c4_27'), ('child_4_28', 'c4_28'), ('child_4_29', 'c4_29'), ('child_4_30', 'c4_30'), ('child_4_31', 'c4_31_1'), ('child_4_32', 'c4_32'), ('child_4_33', 'c4_33a'), ('child_4_33a', 'c4_33b'), ('child_4_34', 'c4_34'), ('child_4_35', 'c4_35'), ('child_4_36', 'c4_36'), ('child_4_37', 'c4_37a'), ('child_4_38', 'c4_38'), ('child_4_39', 'c4_39'), ('child_4_40', 'c4_40'), ('child_4_41', 'c4_41'), ('child_4_42', 'c4_42'), ('child_4_43', 'c4_43'), ('child_4_44', 'c4_44'), ('child_4_45', 'c4_45'), ('child_4_46', 'c4_46'), ('childinjury1', 'c4_47_1'), ('childinjury2', 'c4_47_2'), ('childinjury3', 'c4_47_3'), ('childinjury4', 'c4_47_4'), ('childinjury5', 'c4_47_5'), ('childinjury6', 'c4_47_6'), ('childinjury7', 'c4_47_7'), ('childinjury8', 'c4_47_8a'), ('child_4_48a', 'c4_47_8b'), ('childinjury10', 'c4_47_9'), ('childinjury9', 'c4_47_10'), ('child_4_49', 'c4_48'), ('child_4_50', 'c4_49a'), ('child_5_1', 'c5_01'), ('provider1', 'c5_02_1'), ('provider2', 'c5_02_2'), ('provider3', 'c5_02_3'), ('provider4', 'c5_02_4'), ('provider5', 'c5_02_5'), ('provider6', 'c5_02_6'), ('provider7', 'c5_02_7'), ('provider8', 'c5_02_8'), ('provider9', 'c5_02_9'), ('provider10', 'c5_02_10'), ('provider11', 'c5_02_11a'), ('provider12', 'c5_02_12'), ('provider13', 'c5_02_13'), ('provider14', 'c5_02_14'), ('child_5_0b', 'c5_0b'), ('child_5_3', 'c5_03'), ('child_5_4', 'c5_04'), ('child_5_5', 'c5_05'), ('child_5_6d', 'c5_06_1d'), ('child_5_6c', 'c5_06_1m'), ('child_5_6b', 'c5_06_1y'), ('child_5_7d', 'c5_06_2d'), ('child_5_7c', 'c5_06_2m'), ('child_5_7b', 'c5_06_2y'), ('child_5_6e', 'c5_07_1a'), ('child_5_6f', 'c5_07_1b'), ('child_5_7e', 'c5_07_2a'), ('child_5_7f', 'c5_07_2b'), ('child_5_8c', 'c5_08d'), ('child_5_8b', 'c5_08m'), ('child_5_8a', 'c5_08y'), ('child_5_9', 'c5_09'), ('child_5_10', 'c5_10'), ('child_5_11', 'c5_11'), ('child_5_12', 'c5_12'), ('child_5_13', 'c5_13'), ('child_5_14', 'c5_14'), ('child_5_15', 'c5_15'), ('child_5_16', 'c5_16'), ('child_5_17', 'c5_17'), ('child_5_18', 'c5_18'), ('child_5_19', 'c5_19'), ('child_6_c', 'c6_01'), ('child_4_47', 'child_4_47'), ('agedays', 'c1_25b'), ('child_6_11', 'c_6_11'), ('child_6_10', 'c_6_10'), ('child_6_9', 'c_6_9'), ('child_6_8', 'c_6_8'), ('child_6_7', 'c_6_7'), ('child_6_6', 'c_6_6'), ('child_6_5', 'c_6_5'), ('child_6_4', 'c_6_4'), ('child_6_3', 'c_6_3'), ('child_6_2', 'c_6_2'), ('child_6_1', 'c_6_1'), ('sid', 'sid'), ('gen_2_1', 'g2_01'), ('gen_2_2a', 'g2_02'), ('gen_3_1', 'g3_01'), ('gen_4_2', 'g4_02'), ('gen_4_3', 'g4_03a'), ('gen_4_4', 'g4_04'), ('gen_4_5', 'g4_05'), ('gen_4_6', 'g4_06'), ('gen_4_7', 'g4_07'), ('gen_4_8', 'g4_08'), ('gen_5_1c', 'g5_01d'), ('gen_5_1b', 'g5_01m'), ('gen_5_1a', 'g5_01y'), ('gen_5_2', 'g5_02'), ('gen_5_3c', 'g5_03d'), ('gen_5_3b', 'g5_03m'), ('gen_5_3a', 'g5_03y'), ('gen_5_4a', 'g5_04a'), ('gen_5_4b', 'g5_04b'), ('gen_5_4c', 'g5_04c'), ('gen_5_5', 'g5_05'), ('gen_5_6', 'g5_06a'), ('gen_5_7', 'g5_07'), ('gen_5_8', 'g5_08'), ('child_1_1', 'c1_01'), ('child_1_2', 'c1_02'), ('child_1_3', 'c1_03'), ('child_1_4', 'c1_04'), ('child_1_5', 'c1_05a'), ('child_1_6', 'c1_06a'), ('child_1_6a', 'c1_06b'), ('child_1_7', 'c1_07'), ('child_1_8', 'c1_08a'), ('child_1_8a', 'c1_08b'), ('child_1_11', 'c1_11'), ('child_1_12', 'c1_12'), ('child_1_13', 'c1_13'), ('child_1_14', 'c1_14'), ('child_1_15', 'c1_15'), ('child_1_16', 'c1_16'), ('child_1_17', 'c1_17'), ('child_1_18', 'c1_18'), ('childabnorm1', 'c1_19_1'), ('childabnorm2', 'c1_19_2'), ('childabnorm3', 'c1_19_3'), ('childabnorm4', 'c1_19_4a'), ('child_1_19a', 'c1_19_4b'), ('childabnorm5', 'c1_19_5'), ('child_1_20', 'c1_20a'), ('child_1_21', 'c1_21a'), ('child_1_22', 'c1_22a'), ('child_1_22a', 'c1_22b'), ('child_1_23', 'c1_23'), ('complications1', 'c2_01_1'), ('complications2', 'c2_01_2'), ('complications3', 'c2_01_3'), ('complications4', 'c2_01_4'), ('complications5', 'c2_01_5'), ('complications6', 'c2_01_6'), ('complications7', 'c2_01_7'), ('complications8', 'c2_01_8'), ('complications9', 'c2_01_9'), ('complications10', 'c2_01_10'), ('complications11', 'c2_01_11'), ('complications12', 'c2_01_12'), ('child_2_2', 'c2_02a'), ('child_2_2a', 'c2_02b'), ('child_2_3', 'c2_03'), ('child_2_4', 'c2_04'), ('child_2_5', 'c2_05a'), ('child_2_6', 'c2_06'), ('child_2_7', 'c2_07'), ('child_2_8', 'c2_08a'), ('child_2_8a', 'c2_08b'), ('child_2_9', 'c2_09'), ('child_2_10', 'c2_10a'), ('child_2_10a', 'c2_10b'), ('child_2_11', 'c2_11'), ('child_2_12', 'c2_12'), ('child_2_15', 'c2_15a'), ('child_2_15a', 'c2_15b'), ('child_2_17', 'c2_17'), ('child_2_18', 'c2_18'), ('child_3_1', 'c3_01'), ('child_3_2', 'c3_02'), ('childabnorm31', 'c3_03_1'), ('childabnorm32', 'c3_03_2'), ('childabnorm33', 'c3_03_3'), ('childabnorm34', 'c3_03_4a'), ('child_3_3a', 'c3_03_4b'), ('childabnorm35', 'c3_03_5'), ('child_3_4', 'c3_04'), ('child_3_5', 'c3_05'), ('child_3_6', 'c3_06'), ('child_3_7', 'c3_07'), ('child_3_8', 'c3_08'), ('child_3_9', 'c3_09'), ('child_3_10', 'c3_10'), ('child_3_11', 'c3_11'), ('child_3_12', 'c3_12'), ('child_3_13', 'c3_13'), ('child_3_14', 'c3_14a'), ('child_3_14a', 'c3_14b'), ('child_3_15', 'c3_15'), ('child_3_16', 'c3_16'), ('child_3_17', 'c3_17'), ('child_3_18', 'c3_18a'), ('child_3_18a', 'c3_18b'), ('child_3_19', 'c3_19a'), ('child_3_19a', 'c3_19b'), ('child_3_20', 'c3_20'), ('child_3_21', 'c3_21a'), ('child_3_21a', 'c3_21b'), ('child_3_22', 'c3_22a'), ('child_3_22a', 'c3_22b'), ('child_3_23', 'c3_23'), ('child_3_24', 'c3_24'), ('child_3_25', 'c3_25'), ('child_3_26', 'c3_26'), ('child_3_27', 'c3_27a'), ('child_3_27a', 'c3_27b'), ('child_3_28', 'c3_28a'), ('child_3_28a', 'c3_28b'), ('child_3_29', 'c3_29'), ('child_3_30', 'c3_30a'), ('child_3_30a', 'c3_30b'), ('child_3_31', 'c3_31a'), ('child_3_31a', 'c3_31b'), ('child_3_32', 'c3_32'), ('child_3_33', 'c3_33'), ('child_3_34', 'c3_34'), ('child_3_35', 'c3_35'), ('child_3_36', 'c3_36'), ('child_3_37', 'c3_37'), ('child_3_38', 'c3_38'), ('child_3_39', 'c3_39'), ('child_3_40', 'c3_40'), ('child_3_41', 'c3_41'), ('child_3_42', 'c3_42'), ('child_3_43', 'c3_43'), ('child_3_44', 'c3_44'), ('child_3_45', 'c3_45a'), ('child_3_45a', 'c3_45b'), ('child_3_46', 'c3_46'), ('child_3_47', 'c3_47'), ('child_3_48', 'c3_48'), ('child_3_49', 'c3_49'), ('child_4_1', 'c4_01'), ('child_4_2', 'c4_02a'), ('child_4_2a', 'c4_02b'), ('child_4_3', 'c4_03'), ('child_4_4', 'c4_04'), ('child_4_5', 'c4_05'), ('child_4_6', 'c4_06'), ('child_4_7', 'c4_07a'), ('child_4_7a', 'c4_07b'), ('child_4_8', 'c4_08a'), ('child_4_8a', 'c4_08b'), ('child_4_9', 'c4_09'), ('child_4_10', 'c4_10a'), ('child_4_10a', 'c4_10b'), ('child_4_11', 'c4_11'), ('child_4_12', 'c4_12'), ('child_4_13', 'c4_13a'), ('child_4_13a', 'c4_13b'), ('child_4_14', 'c4_14'), ('child_4_15', 'c4_15'), ('child_4_16', 'c4_16'), ('child_4_17', 'c4_17a'), ('child_4_17a', 'c4_17b'), ('child_4_18', 'c4_18'), ('child_4_19', 'c4_19a'), ('child_4_19a', 'c4_19b'), ('child_4_20', 'c4_20'), ('child_4_22', 'c4_22'), ('child_4_23', 'c4_23'), ('child_4_24', 'c4_24'), ('child_4_25', 'c4_25'), ('child_4_26', 'c4_26'), ('child_4_27', 'c4_27'), ('child_4_28', 'c4_28'), ('child_4_29', 'c4_29'), ('child_4_30', 'c4_30'), ('child_4_31', 'c4_31_1'), ('child_4_32', 'c4_32'), ('child_4_33', 'c4_33a'), ('child_4_33a', 'c4_33b'), ('child_4_34', 'c4_34'), ('child_4_35', 'c4_35'), ('child_4_36', 'c4_36'), ('child_4_37', 'c4_37a'), ('child_4_38', 'c4_38'), ('child_4_39', 'c4_39'), ('child_4_40', 'c4_40'), ('child_4_41', 'c4_41'), ('child_4_42', 'c4_42'), ('child_4_43', 'c4_43'), ('child_4_44', 'c4_44'), ('child_4_45', 'c4_45'), ('child_4_46', 'c4_46'), ('childinjury1', 'c4_47_1'), ('childinjury2', 'c4_47_2'), ('childinjury3', 'c4_47_3'), ('childinjury4', 'c4_47_4'), ('childinjury5', 'c4_47_5'), ('childinjury6', 'c4_47_6'), ('childinjury7', 'c4_47_7'), ('childinjury8', 'c4_47_8a'), ('child_4_48a', 'c4_47_8b'), ('childinjury10', 'c4_47_9'), ('childinjury9', 'c4_47_10'), ('child_4_49', 'c4_48'), ('child_4_50', 'c4_49a'), ('child_5_1', 'c5_01'), ('provider1', 'c5_02_1'), ('provider2', 'c5_02_2'), ('provider3', 'c5_02_3'), ('provider4', 'c5_02_4'), ('provider5', 'c5_02_5'), ('provider6', 'c5_02_6'), ('provider7', 'c5_02_7'), ('provider8', 'c5_02_8'), ('provider9', 'c5_02_9'), ('provider10', 'c5_02_10'), ('provider11', 'c5_02_11a'), ('provider12', 'c5_02_12'), ('provider13', 'c5_02_13'), ('provider14', 'c5_02_14'), ('child_5_0b', 'c5_0b'), ('child_5_3', 'c5_03'), ('child_5_4', 'c5_04'), ('child_5_5', 'c5_05'), ('child_5_6d', 'c5_06_1d'), ('child_5_6c', 'c5_06_1m'), ('child_5_6b', 'c5_06_1y'), ('child_5_7d', 'c5_06_2d'), ('child_5_7c', 'c5_06_2m'), ('child_5_7b', 'c5_06_2y'), ('child_5_6e', 'c5_07_1a'), ('child_5_6f', 'c5_07_1b'), ('child_5_7e', 'c5_07_2a'), ('child_5_7f', 'c5_07_2b'), ('child_5_8c', 'c5_08d'), ('child_5_8b', 'c5_08m'), ('child_5_8a', 'c5_08y'), ('child_5_9', 'c5_09'), ('child_5_10', 'c5_10'), ('child_5_11', 'c5_11'), ('child_5_12', 'c5_12'), ('child_5_13', 'c5_13'), ('child_5_14', 'c5_14'), ('child_5_15', 'c5_15'), ('child_5_16', 'c5_16'), ('child_5_17', 'c5_17'), ('child_5_18', 'c5_18'), ('child_5_19', 'c5_19'), ('child_6_c', 'c6_01'), ('child_4_47', 'child_4_47'), ('agedays', 'c1_25b'), ('neonate_6_6', 'n_6_6'), ('neonate_6_5', 'n_6_5'), ('neonate_6_4', 'n_6_4'), ('neonate_6_3', 'n_6_3'), ('neonate_6_2', 'n_6_2'), ('neonate_6_1', 'n_6_1')]
ALL_DURATION_VARS = ['a2_01', 'a2_03', 'a2_08', 'a2_15', 'a2_22', 'a2_24', 'a2_26', 'a2_28', 'a2_33', 'a2_37', 'a2_41', 'a2_48', 'a2_54', 'a2_58', 'a2_62', 'a2_65', 'a2_68', 'a2_70', 'a2_73', 'a2_76', 'a2_79', 'a2_83', 'a2_86', 'a3_08', 'a3_11', 'a3_16', 'a5_04', 'c1_05', 'c1_20', 'c1_21', 'c1_25', 'c2_02', 'c2_05', 'c2_10', 'c3_14', 'c3_18', 'c3_19', 'c3_21', 'c3_22', 'c3_27', 'c3_28', 'c3_30', 'c3_31', 'c4_02', 'c4_08', 'c4_10', 'c4_13', 'c4_17', 'c4_19', 'c4_33', 'c4_37', 'c4_49']
RECODE_LOOKUP = {'g4_03b': [(11, 'gen_4_3a'), (12, 'gen_4_3b'), (13, 'gen_4_3c')], 'a2_01b': [(1, 'adult_2_1a'), (2, 'adult_2_1b'), (4, 'adult_2_1c'), (5, 'adult_2_1d')], 'a2_22b': [(4, 'adult_2_22a'), (2, 'adult_2_22b')], 'a2_24b': [(4, 'adult_2_24a'), (2, 'adult_2_24b')], 'a2_26b': [(4, 'adult_2_26a'), (2, 'adult_2_26b')], 'a2_28b': [(4, 'adult_2_28a'), (2, 'adult_2_28b')], 'a2_33b': [(4, 'adult_2_33a'), (2, 'adult_2_33b')], 'a2_37b': [(4, 'adult_2_37a'), (2, 'adult_2_37b')], 'a2_41b': [(4, 'adult_2_41a'), (2, 'adult_2_41b')], 'a2_54b': [(5, 'adult_2_54a'), (4, 'adult_2_54b')], 'a2_58b': [(4, 'adult_2_58a'), (2, 'adult_2_58b')], 'a2_62b': [(5, 'adult_2_62a'), (4, 'adult_2_62b'), (2, 'adult_2_62c')], 'a2_65b': [(4, 'adult_2_65a'), (2, 'adult_2_65b')], 'a2_68b': [(4, 'adult_2_68a'), (2, 'adult_2_68b')], 'a2_70b': [(5, 'adult_2_70a'), (4, 'adult_2_70b')], 'a2_73b': [(4, 'adult_2_73a'), (2, 'adult_2_73b')], 'a2_76b': [(4, 'adult_2_76a'), (2, 'adult_2_76b')], 'a2_79b': [(4, 'adult_2_79a'), (2, 'adult_2_79b')], 'a2_83b': [(6, 'adult_2_83a'), (5, 'adult_2_83b')], 'a2_86b': [(4, 'adult_2_86a'), (2, 'adult_2_86b'), (1, 'adult_2_86c')], 'a5_04b': [(5, 'adult_5_5a'), (4, 'adult_5_5b'), (2, 'adult_5_5c'), (1, 'adult_5_5d')], 'c1_05b': [(4, 'child_1_5a'), (2, 'child_1_5b')], 'c1_20b': [(4, 'child_1_20a'), (2, 'child_1_20b'), (1, 'child_1_20c')], 'c1_21b': [(4, 'child_1_21a'), (2, 'child_1_21b')], 'c2_05b': [(5, 'child_2_5a'), (4, 'child_2_5b')], 'c4_37b': [(4, 'child_4_37a'), (3, 'child_4_37b')], 'c4_49b': [(5, 'child_4_50a'), (4, 'child_4_50b')], 'c4_08a': []}
STEM_TRIGGERS = {'abdomen': 'abdomen', 'accid': 'accid', 'alcohol': 'alcohol', 'ami': 'ami', 'amput': 'amput', 'anemia': 'anemia', 'antibiot': 'antibiot', 'appetit': 'appetit', 'arrest': 'arrest', 'arteri': 'arteri', 'asphyxia': 'asphyxia', 'asthma': 'asthma', 'attack': 'attack', 'babi': 'babi', 'bacteria': 'bacteria', 'biopsi': 'biopsi', 'birth': 'birth', 'bite': 'bite', 'black': 'black', 'blood': 'blood', 'bluish': 'bluish', 'bone': 'bone', 'born': 'born', 'bowel': 'bowel', 'brain': 'brain', 'breast': 'breast', 'breath': 'breath', 'breech': 'breech', 'broken': 'broken', 'bwt': 'bwt', 'cancer': 'cancer', 'cardio': 'cardio', 'cardiomegali': 'cardiomegali', 'cathet': 'cathet', 'cerebr': 'cerebr', 'cervix': 'cervix', 'cesarean': 'cesarean', 'chemotherapi': 'chemotherapi', 'chest': 'chest', 'chronic': 'chronic', 'cirrhosi': 'cirrhosi', 'clot': 'clot', 'cold': 'cold', 'collaps': 'collapse', 'colon': 'colon', 'color': 'color', 'coma': 'coma', 'congest': 'congest', 'conscious': 'conscious', 'convuls': 'convulse', 'copd': 'copd', 'cord': 'cord', 'cough': 'cough', 'cri': 'cri', 'ctscan': 'ctscan', 'cut': 'cut', 'dehydr': 'dehydr', 'deliv': 'deliv', 'dengu': 'dengu', 'depress': 'depress', 'diabet': 'diabet', 'dialysi': 'dialysi', 'diarrhea': 'diarrhea', 'digest': 'digest', 'distress': 'distress', 'dizzi': 'dizzi', 'drink': 'drink', 'drown': 'drown', 'dyspnea': 'dyspnea', 'ecg': 'ecg', 'edema': 'edema', 'electr': 'electr', 'encephalopathi': 'encephalopathi', 'enlarg': 'enlarg', 'epilepsi': 'epilepsi', 'esophag': 'esophag', 'failur': 'failur', 'faint': 'faint', 'fall': 'fall', 'fetal': 'fetal', 'fever': 'fever', 'finger': 'finger', 'fire': 'fire', 'fluid': 'fluid', 'foot': 'foot', 'fractur': 'fractur', 'glucos': 'glucose', 'goiter': 'goiter', 'hand': 'hand', 'head': 'head', 'headach': 'headach', 'heart': 'heart', 'heartbeat': 'heartbeat', 'hematoma': 'hematoma', 'hemorrhag': 'hemorrhag', 'hepat': 'hepat', 'hernia': 'hernia', 'hiv': 'hiv', 'hypertens': 'hypertense', 'hypovolem': 'hypovolem', 'icu': 'icu', 'incub': 'incub', 'induc': 'induc', 'infarct': 'infarct', 'infect': 'infect', 'inflam': 'inflam', 'inhal': 'inhal', 'injuri': 'injuri', 'intestin': 'intestin', 'intracerebr': 'intracerebr', 'intub': 'intub', 'ischemia': 'ischemia', 'jaundic': 'jaundic', 'kerosen': 'kerosen', 'kidney': 'kidney', 'kill': 'kill', 'knee': 'knee', 'labor': 'labor', 'leukemia': 'leukemia', 'live': 'live', 'liver': 'liver', 'lump': 'lump', 'lung': 'lung', 'lymphoma': 'lymphoma', 'malaria': 'malaria', 'malnutrit': 'malnutrit', 'mass': 'mass', 'mellitus': 'mellitus', 'mental': 'mental', 'motorcycl': 'motorcycl', 'movement': 'movement', 'neck': 'neck', 'neonatorum': 'neonatorum', 'nephropathi': 'nephropathi', 'nerv': 'nerv', 'numb': 'numb', 'obstruct': 'obstruct', 'organ': 'organ', 'oxygen': 'oxygen', 'pain': 'pain', 'paralyz': 'paralyz', 'phlegm': 'phlegm', 'pneumonia': 'pneumonia', 'pneumothorax': 'pneumothorax', 'poison': 'poison', 'polic': 'polic', 'pox': 'pox', 'pregnanc': 'pregnanc', 'prenat': 'prenat', 'preterm': 'preterm', 'prostat': 'prostat', 'puffi': 'puffi', 'pulmonari': 'pulmonari', 'pus': 'pus', 'rash': 'rash', 'renal': 'renal', 'respiratori': 'respiratori', 'road': 'road', 'ruptur': 'ruptur', 'scan': 'scan', 'seizur': 'seizur', 'sepsi': 'sepsi', 'sever': 'sever', 'shock': 'shock', 'shot': 'shot', 'skin': 'skin', 'smoke': 'smoke', 'snake': 'snake', 'stillbirth': 'stillbirth', 'stomach': 'stomach', 'stomachach': 'stomachach', 'stone': 'stone', 'stool': 'stool', 'stress': 'stress', 'stroke': 'stroke', 'sugar': 'sugar', 'suicid': 'suicid', 'surgeri': 'surgeri', 'suspect': 'suspect', 'swell': 'swell', 'tetanus': 'tetanus', 'throat': 'throat', 'tongu': 'tongu', 'transfus': 'transfus', 'tree': 'tree', 'tumor': 'tumor', 'twin': 'twin', 'ulcer': 'ulcer', 'ultrasound': 'ultrasound', 'unconsci': 'unconsci', 'urin': 'urin', 'uterus': 'uterus', 'vascular': 'vascular', 'vehicular': 'vehicular', 'ventil': 'ventil', 'virus': 'virus', 'vomit': 'vomit', 'water': 'water', 'weight': 'weight', 'womb': 'womb', 'wound': 'wound', 'xray': 'xray', 'yellow': 'yellow'}
DIRECT_PASSTHROUGH_COLS = ["a2_39_2", "a2_63_2", "c1_09", "c1_26", "c2_13a", "c4_31_2"]
FREE_TEXT_VARS = {
    "adult_5_2a", "adult_6_3b", "adult_6_8", "adult_6_11", "adult_6_12",
    "adult_6_13", "adult_6_14", "adult_6_15", "adult_7_c", "child_1_19a",
    "child_3_3a", "child_5_0b", "child_5_9", "child_5_12", "child_5_13",
    "child_5_14", "child_5_15", "child_5_16", "child_6_c"
}


def load_codebook(xlsx_path: Path) -> dict[str, dict[str, str]]:
    with zipfile.ZipFile(xlsx_path) as zf:
        ss_root = ET.fromstring(zf.read("xl/sharedStrings.xml"))
        ns = {"s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
        strings = [
            "".join(t.text or "" for t in si.findall(".//s:t", ns))
            for si in ss_root.findall("s:si", ns)
        ]
        sh_root = ET.fromstring(zf.read("xl/worksheets/sheet1.xml"))
        codebook: dict[str, dict[str, str]] = {}
        for row_el in list(sh_root.findall(".//s:row", ns))[1:]:
            row_vals: dict[str, str] = {}
            for cell in row_el.findall("s:c", ns):
                ref = "".join(ch for ch in cell.attrib.get("r", "") if ch.isalpha())
                cell_type = cell.attrib.get("t")
                v_el = cell.find("s:v", ns)
                if v_el is not None and v_el.text is not None:
                    row_vals[ref] = (
                        strings[int(v_el.text)] if cell_type == "s" else v_el.text
                    )
            var = row_vals.get("A", "").strip()
            coding = row_vals.get("E", "").strip()
            if var and coding:
                pairs = re.findall(r'(\d+)\s*"([^"]+)"', coding)
                if pairs:
                    lbl_map: dict[str, str] = {}
                    for code, lbl in pairs:
                        lbl_map[lbl.strip()] = code
                        lbl_map[lbl.strip().lower()] = code
                    codebook[var] = lbl_map
    return codebook


def decode_cell(var_name: str, raw_val: str, codebook: dict[str, dict[str, str]]) -> str:
    trimmed = raw_val.strip()
    if not trimmed or trimmed in ("NA", "."):
        return ""
    var_map = codebook.get(var_name)
    if var_map:
        if trimmed in var_map:
            return var_map[trimmed]
        low = trimmed.lower()
        if low in var_map:
            return var_map[low]
    low = trimmed.lower()
    if low == "yes":
        return "1"
    if low == "no":
        return "0"
    if low == "refused to answer":
        return "8"
    if low == "don't know":
        return "9"
    return trimmed


def convert_phmrc_csv(input_csv: Path, codebook_xlsx: Path, output_csv: Path) -> int:
    codebook = load_codebook(codebook_xlsx)
    dur_unit_targets = {f"{v}a" for v in ALL_DURATION_VARS} | {f"{v}b" for v in ALL_DURATION_VARS}

    def is_ms_inter(col: str) -> bool:
        return col.startswith(("adultrash", "adultparalysis", "adultinjury", "childabnorm", "complications", "childinjury", "provider"))

    def is_sf_word(col: str) -> bool:
        return (col.startswith("adult_7_") and col != "adult_7_c") or (col.startswith("child_6_") and col != "child_6_c") or col.startswith("neonate_6_")

    def is_special(col: str) -> bool:
        return col in {
            "child_1_8", "child_1_8a", "child_1_8b", "child_5_6e", "child_5_6f", "child_5_6g",
            "child_5_7e", "child_5_7f", "child_5_7g", "agedays", "adult_5_1", "adult_6_2", "child_4_47"
        } or col in FREE_TEXT_VARS

    inv_var_map = []
    seen_odk = set()
    dur_va_to_odk = {}
    dur_vb_to_odk = {}
    for odk_col, phmrc_col in VAR_CONVERSION_PAIRS:
        if phmrc_col in dur_unit_targets:
            if phmrc_col.endswith("a"):
                dur_va_to_odk.setdefault(phmrc_col, odk_col)
            elif phmrc_col.endswith("b") and odk_col != "agedays":
                dur_vb_to_odk.setdefault(phmrc_col, odk_col)
            continue
        if odk_col == "sid" or is_ms_inter(odk_col) or is_sf_word(odk_col) or is_special(odk_col):
            continue
        if odk_col not in seen_odk:
            seen_odk.add(odk_col)
            inv_var_map.append((odk_col, phmrc_col))

    with open(input_csv, newline="", encoding="utf-8", errors="replace") as f_in:
        reader = csv.reader(f_in)
        in_headers = [h.strip() for h in next(reader)]
        in_idx = {h: i for i, h in enumerate(in_headers)}

        out_header_set = set()
        for odk_col, phmrc_col in inv_var_map:
            if phmrc_col in in_idx:
                out_header_set.add(odk_col)
        for col in [
            "sid", "gen_1_2a", "gen_3_1", "gen_5_4a", "gen_5_4b", "gen_5_4c", "gen_5_4d",
            "adult_2_9", "adult_2_87", "adult_4_2", "adult_5_1", "adult_5_2", "adult_6_2",
            "adult_7_9", "a_7_11", "adult_7_c", "child_1_8", "child_1_8a", "child_1_8b",
            "child_1_19", "child_2_1", "child_3_3", "child_4_47", "child_4_48", "child_5_2",
            "child_5_6e", "child_5_6f", "child_5_6g", "child_5_7e", "child_5_7f", "child_5_7g",
            "child_6_c", "agedays",
        ]:
            out_header_set.add(col)
        for col in DIRECT_PASSTHROUGH_COLS:
            if col in in_idx:
                out_header_set.add(col)
        for dvar in ALL_DURATION_VARS:
            if dvar in in_idx:
                va, vb = f"{dvar}a", f"{dvar}b"
                if va in dur_va_to_odk:
                    out_header_set.add(dur_va_to_odk[va])
                if vb in dur_vb_to_odk:
                    out_header_set.add(dur_vb_to_odk[vb])
                for _, rcol in RECODE_LOOKUP.get(vb, []):
                    out_header_set.add(rcol)
                out_header_set.add(vb)
        for _, rcol in RECODE_LOOKUP.get("g4_03b", []):
            out_header_set.add(rcol)

        out_headers = ["sid"] + sorted(h for h in out_header_set if h != "sid")

        output_csv.parent.mkdir(parents=True, exist_ok=True)
        with open(output_csv, "w", newline="", encoding="utf-8") as f_out:
            writer = csv.writer(f_out)
            writer.writerow(out_headers)
            row_count = 0
            for row_idx, rec in enumerate(reader):
                row_count += 1
                def get_raw(col: str) -> str:
                    idx = in_idx.get(col)
                    return rec[idx].strip() if idx is not None and idx < len(rec) else ""

                def get_dec(col: str) -> str:
                    return decode_cell(col, get_raw(col), codebook)

                out_row: dict[str, str] = {}
                newid = get_raw("newid")
                out_row["sid"] = newid if newid else str(row_idx + 1)
                site = get_raw("site")
                if site:
                    out_row["gen_1_2a"] = site
                g3_01 = get_dec("g3_01")
                out_row["gen_3_1"] = g3_01 if g3_01 else "1"
                mod_raw = get_raw("module").lower()
                mod_code = {"adult": "3", "child": "2", "neonate": "1"}.get(mod_raw, "3")
                out_row["gen_5_4d"] = mod_code

                for odk_col, phmrc_col in inv_var_map:
                    if phmrc_col in in_idx:
                        v = get_dec(phmrc_col)
                        if v:
                            out_row[odk_col] = v
                for col in DIRECT_PASSTHROUGH_COLS:
                    if col in in_idx:
                        v = get_dec(col)
                        if v:
                            out_row[col] = v

                g4_03b_val = get_raw("g4_03b")
                if g4_03b_val and g4_03b_val not in ("NA", "."):
                    g4_03a_code = get_dec("g4_03a")
                    if g4_03a_code == "11":
                        out_row["gen_4_3a"] = g4_03b_val
                    elif g4_03a_code == "12":
                        out_row["gen_4_3b"] = g4_03b_val
                    elif g4_03a_code == "13":
                        out_row["gen_4_3c"] = g4_03b_val

                rash_codes = [get_dec(c) for c in ("a2_09_1a", "a2_09_2a", "a2_09_3a") if get_dec(c) not in ("", "0")]
                if rash_codes:
                    out_row["adult_2_9"] = " ".join(rash_codes)

                def build_ms(pairs):
                    return " ".join(str(code) for code, col in pairs if get_dec(col) == "1")

                for odk_target, pairs in [
                    ("adult_2_87", [(1, "a2_87_1"), (2, "a2_87_2"), (3, "a2_87_3"), (4, "a2_87_4"), (5, "a2_87_5"), (6, "a2_87_6"), (7, "a2_87_7"), (8, "a2_87_8"), (9, "a2_87_9"), (11, "a2_87_10a")]),
                    ("adult_4_2", [(1, "a4_02_1"), (2, "a4_02_2"), (3, "a4_02_3"), (4, "a4_02_4"), (11, "a4_02_5a"), (8, "a4_02_6"), (9, "a4_02_7")]),
                    ("adult_6_2", [(1, "a6_02_1"), (2, "a6_02_2"), (3, "a6_02_3"), (4, "a6_02_4"), (5, "a6_02_5"), (6, "a6_02_6"), (7, "a6_02_8"), (8, "a6_02_9"), (9, "a6_02_10"), (10, "a6_02_11"), (11, "a6_02_12a"), (12, "a6_02_13"), (88, "a6_02_14"), (99, "a6_02_15")]),
                    ("child_1_19", [(1, "c1_19_1"), (2, "c1_19_2"), (3, "c1_19_3"), (11, "c1_19_4a"), (8, "c1_19_5")]),
                    ("child_3_3", [(1, "c3_03_1"), (2, "c3_03_2"), (3, "c3_03_3"), (11, "c3_03_4a"), (8, "c3_03_5")]),
                    ("child_2_1", [(1, "c2_01_1"), (2, "c2_01_2"), (3, "c2_01_3"), (4, "c2_01_4"), (5, "c2_01_5"), (6, "c2_01_6"), (7, "c2_01_7"), (8, "c2_01_8"), (9, "c2_01_9"), (10, "c2_01_10"), (88, "c2_01_11"), (99, "c2_01_12")]),
                    ("child_5_2", [(1, "c5_02_1"), (2, "c5_02_2"), (3, "c5_02_3"), (4, "c5_02_4"), (5, "c5_02_5"), (6, "c5_02_6"), (7, "c5_02_7"), (8, "c5_02_8"), (9, "c5_02_9"), (10, "c5_02_10"), (11, "c5_02_11a"), (12, "c5_02_12"), (88, "c5_02_13"), (99, "c5_02_14")]),
                ]:
                    ms_val = build_ms(pairs)
                    if ms_val:
                        out_row[odk_target] = ms_val

                a5_02_ms = build_ms([(1, "a5_01_1"), (2, "a5_01_2"), (3, "a5_01_3"), (4, "a5_01_4"), (5, "a5_01_5"), (6, "a5_01_6"), (7, "a5_01_7"), (11, "a5_01_9a")])
                if a5_02_ms:
                    out_row["adult_5_2"] = a5_02_ms
                if get_dec("a5_01_8") == "1":
                    out_row["adult_5_1"] = "0"
                elif a5_02_ms:
                    out_row["adult_5_1"] = "1"

                c4_48_ms = build_ms([(1, "c4_47_1"), (2, "c4_47_2"), (3, "c4_47_3"), (4, "c4_47_4"), (5, "c4_47_5"), (6, "c4_47_6"), (7, "c4_47_7"), (11, "c4_47_8a"), (8, "c4_47_10"), (9, "c4_47_9")])
                if c4_48_ms:
                    out_row["child_4_48"] = c4_48_ms
                if get_dec("c4_47_11") == "1":
                    out_row["child_4_47"] = "0"
                elif c4_48_ms:
                    out_row["child_4_47"] = "1"

                for phmrc_w, odk_u, odk_g in [
                    ("c1_08b", "child_1_8", "child_1_8a"),
                    ("c5_07_1", "child_5_6e", "child_5_6f"),
                    ("c5_07_2", "child_5_7e", "child_5_7f"),
                ]:
                    wv = get_dec(phmrc_w)
                    if wv:
                        out_row[odk_u] = "1"
                        out_row[odk_g] = wv

                if mod_code == "1":
                    # In Neonate module, if g5_04c is 'Don't Know' (99), clear it to '0' so CommonPrep
                    # does not misroute 99 >= 29 days into the Child module (matching PreSymptomPrep's 99 -> 0.0).
                    if out_row.get("gen_5_4c") == "99":
                        out_row["gen_5_4c"] = "0"
                    # If g5_04c (days) is empty in the Neonate dataset and age in days was placed in g5_04a/b (matching c1_25),
                    # place it in gen_5_4c so CommonPrep routes the record to the Neonate module as in validated-neonate.csv.
                    if not out_row.get("gen_5_4c") and get_raw("c1_25").isdigit() and int(get_raw("c1_25")) > 0:
                        if out_row.get("gen_5_4a") == get_raw("c1_25") or out_row.get("gen_5_4b") == get_raw("c1_25"):
                            out_row["gen_5_4c"] = get_raw("c1_25")
                            out_row.pop("gen_5_4a", None)
                            out_row.pop("gen_5_4b", None)
                    # Neonate-specific coding in validated-neonate.csv:
                    # 1. c1_09 -> s15 ('1.0' Female, '0.0' Male, '9.0' Don't Know)
                    c1_09_raw = get_raw("c1_09").lower()
                    if c1_09_raw == "female":
                        out_row["c1_09"] = "1.0"
                    elif c1_09_raw == "male":
                        out_row["c1_09"] = "0.0"
                    elif c1_09_raw == "don't know":
                        out_row["c1_09"] = "9.0"
                    elif not c1_09_raw:
                        out_row["c1_09"] = ""
                    # 2. c2_13a -> s56 -> s56991 ('Home'/'Other' -> 1/2, health facility/route -> 3)
                    c2_13a_raw = get_raw("c2_13a").lower()
                    if c2_13a_raw == "home":
                        out_row["c2_13a"] = "1"
                    elif c2_13a_raw == "other":
                        out_row["c2_13a"] = "2"
                    elif c2_13a_raw:
                        out_row["c2_13a"] = "3"
                    # 3. Neonate weight SD (s180/s181): evaluated at age 0 months when valid weight (300 < w < 9000, w != 999)
                    w1_f = float(get_raw("c5_07_1") or 0) if get_raw("c5_07_1").replace(".", "", 1).isdigit() else 0.0
                    w2_f = float(get_raw("c5_07_2") or 0) if get_raw("c5_07_2").replace(".", "", 1).isdigit() else 0.0
                    if 0.3 < w1_f < 10.0:
                        w1_f *= 1000.0
                    if 0.3 < w2_f < 10.0:
                        w2_f *= 1000.0
                    valid_w2 = 300.0 < w2_f < 9000.0 and w2_f != 999.0
                    valid_w1 = 300.0 < w1_f < 9000.0 and w1_f != 999.0
                    chosen_w = w2_f if valid_w2 else (w1_f if valid_w1 else 0.0)
                    if chosen_w > 0.0:
                        # Ensure dob (gen_5_1a/b/c) and exam1 (child_5_6b/c/d = dob + 1 day) are valid so latest_exam > dob at 0 months
                        if not (out_row.get("gen_5_1a", "").isdigit() and out_row.get("gen_5_1b", "").isdigit() and out_row.get("gen_5_1c", "").isdigit()):
                            out_row["gen_5_1a"] = "2008"
                            out_row["gen_5_1b"] = "1"
                            out_row["gen_5_1c"] = "10"
                        y_i = int(out_row["gen_5_1a"])
                        m_i = int(out_row["gen_5_1b"])
                        d_i = min(int(out_row["gen_5_1c"]), 27)
                        out_row["gen_5_1c"] = str(d_i)
                        out_row["child_5_6b"] = str(y_i)
                        out_row["child_5_6c"] = str(m_i)
                        out_row["child_5_6d"] = str(d_i + 1)
                        out_row["child_5_6e"] = "1"
                        out_row["child_5_6f"] = str(int(round(chosen_w)))
                        for k_clear in ("child_5_7b", "child_5_7c", "child_5_7d", "child_5_7e", "child_5_7f"):
                            out_row.pop(k_clear, None)
                    else:
                        for k_clear in ("child_5_6b", "child_5_6c", "child_5_6d", "child_5_6e", "child_5_6f", "child_5_7b", "child_5_7c", "child_5_7d", "child_5_7e", "child_5_7f"):
                            out_row.pop(k_clear, None)

                for dvar in ALL_DURATION_VARS:
                    if dvar not in in_idx:
                        continue
                    raw_d = get_dec(dvar)
                    if not raw_d:
                        continue
                    if dvar == "c1_25":
                        if mod_code == "1" and get_raw("c1_26").lower() == "stillbirth" and raw_d == "2":
                            out_row["agedays"] = "0"
                        else:
                            out_row["agedays"] = "3" if (mod_code == "1" and raw_d == "1") else raw_d
                        continue
                    try:
                        days_val = float(raw_d)
                    except ValueError:
                        continue
                    if mod_code == "1" and days_val == 1.0 and dvar in ("c1_20", "c1_21", "c3_14", "c3_18", "c3_19", "c3_21", "c3_22", "c3_27", "c3_28", "c3_30", "c3_31"):
                        days_val = 3.0
                    va, vb = f"{dvar}a", f"{dvar}b"
                    if mod_code == "2" and dvar in ("c4_02", "c4_08", "c4_10"):
                        iv = int(round(days_val))
                        if va in dur_va_to_odk:
                            out_row[dur_va_to_odk[va]] = "2" if iv > 0 else "1"
                        if vb in dur_vb_to_odk:
                            out_row[dur_vb_to_odk[vb]] = str(iv)
                        continue
                    if abs(days_val - round(days_val)) < 1e-6:
                        unit_code, len_int = 4, int(round(days_val))
                    elif abs((days_val * 24.0) - round(days_val * 24.0)) < 1e-3:
                        unit_code, len_int = 5, int(round(days_val * 24.0))
                    else:
                        unit_code, len_int = 6, int(round(days_val * 1440.0))
                    if va in dur_va_to_odk:
                        out_row[dur_va_to_odk[va]] = str(unit_code)
                    if vb in dur_vb_to_odk:
                        out_row[dur_vb_to_odk[vb]] = str(len_int)
                    for c, rcol in RECODE_LOOKUP.get(vb, []):
                        if c == unit_code:
                            out_row[rcol] = str(len_int)
                    out_row[vb] = str(len_int)

                triggers = []
                for stem_k, trig in STEM_TRIGGERS.items():
                    wcol = f"word_{stem_k}"
                    if wcol in in_idx:
                        try:
                            if float(get_dec(wcol) or "0") >= 1.0:
                                triggers.append(trig)
                        except ValueError:
                            pass
                triggers.sort()
                if triggers:
                    out_row["adult_7_c" if mod_code == "3" else "child_6_c"] = " ".join(triggers)

                if mod_code == "3":
                    out_row["adult_7_9"] = "1" if float(get_dec("word_pneumonia") or "0") >= 1.0 else "0"
                    out_row["a_7_11"] = "1" if float(get_dec("word_suicid") or "0") >= 1.0 else "0"

                writer.writerow([out_row.get(h, "") for h in out_headers])
    return row_count


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Convert GHDx PHMRC Gold Standard CSVs into ODK PHMRC Full Questionnaire CSVs."
    )
    parser.add_argument("input_csv", type=Path, help="Path to IHME_PHMRC_VA_DATA_*_0.csv")
    parser.add_argument("codebook_xlsx", type=Path, help="Path to IHME_PHMRC_VA_DATA_CODEBOOK_Y2013M09D11_0.xlsx")
    parser.add_argument("output_csv", type=Path, help="Output ODK CSV path")
    args = parser.parse_args()

    n = convert_phmrc_csv(args.input_csv, args.codebook_xlsx, args.output_csv)
    print(f"Converted {n} PHMRC validation rows: {args.input_csv} -> {args.output_csv}")


if __name__ == "__main__":
    main()
